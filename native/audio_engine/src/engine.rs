//! Main engine coordinator.
//!
//! Two pipelines:
//!   OUTGOING: Mic -> Deepgram(ru) -> Translate(ru->en) -> TTS(en) -> Speakers
//!   INCOMING: BlackHole 16ch -> Deepgram(en) -> Translate(en->ru) -> TTS(ru) -> Speakers

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use crossbeam_channel::{bounded, Sender};
use log::{debug, error, info, warn};

use crate::audio;
use crate::audio::capture::{AudioCapture, AudioChunk};
use crate::audio::playback::AudioPlayback;
use crate::protocol::{Command, Event};
use crate::stt::{DeepgramStt, UnifiedSttSession};
use crate::tracelog;
use crate::translation::{TranslationDirection, TranslationEngine};
use crate::tts::TtsEngine;

// ---------------------------------------------------------------------------
// EngineConfig
// ---------------------------------------------------------------------------

pub struct EngineConfig {
    pub deepgram_api_key: String,
    pub groq_api_key: String,
    pub yandex_api_key: String,
    pub yandex_folder_id: String,
    pub translation_provider: String,
    pub litellm_base_url: String,
    pub litellm_api_key: String,
    pub litellm_model: String,
    pub stt_provider: String,
    pub whisper_model: String,
    pub local_mt_ruen: String,
    pub local_mt_enru: String,
    pub tts_en_model: String,
    pub tts_en_config: String,
    pub tts_ru_model: String,
    pub tts_ru_config: String,
    pub mic_device: String,
    pub speaker_device: String,
    pub meet_input_device: String,
    pub meet_output_device: String,
    pub sample_rate: u32,
    pub endpointing_ms: u32,
    pub utterance_end_ms: u32,
    pub my_language: String,
    pub their_language: String,
}

impl EngineConfig {
    pub fn from_env() -> Self {
        let base = std::env::var("TRANSLATOR_MODELS_DIR").unwrap_or_else(|_| "./models".into());
        // Read API keys from environment variables (set by Elixir from settings.json)
        let dg_key = std::env::var("DEEPGRAM_API_KEY").unwrap_or_default();
        let groq_key = std::env::var("GROQ_API_KEY").unwrap_or_default();
        let yandex_key = std::env::var("YANDEX_API_KEY").unwrap_or_default();
        let yandex_folder_id = std::env::var("YANDEX_FOLDER_ID").unwrap_or_default();
        let translation_provider = std::env::var("TRANSLATOR_PROVIDER").unwrap_or_else(|_| "auto".into());
        
        eprintln!("[ENGINE] DEEPGRAM_API_KEY from env: {}... (len={})", 
            if dg_key.len() >= 4 { &dg_key[..4] } else { "?" }, dg_key.len());
        eprintln!("[ENGINE] GROQ_API_KEY from env: {}... (len={})", 
            if groq_key.len() >= 4 { &groq_key[..4] } else { "?" }, groq_key.len());
        eprintln!("[ENGINE] YANDEX_API_KEY from env: {}... (len={})", 
            if yandex_key.len() >= 4 { &yandex_key[..4] } else { "?" }, yandex_key.len());
        eprintln!("[ENGINE] TRANSLATOR_PROVIDER='{}'", translation_provider);

        // Log device env vars for debugging
        let mic = std::env::var("TRANSLATOR_MIC_DEVICE").unwrap_or_default();
        let speaker = std::env::var("TRANSLATOR_SPEAKER_DEVICE").unwrap_or_default();
        let meet_in = std::env::var("TRANSLATOR_MEET_INPUT").unwrap_or_default();
        let meet_out = std::env::var("TRANSLATOR_MEET_OUTPUT").unwrap_or_default();
        eprintln!("[ENGINE] TRANSLATOR_MIC_DEVICE='{}'", mic);
        eprintln!("[ENGINE] TRANSLATOR_SPEAKER_DEVICE='{}'", speaker);
        eprintln!("[ENGINE] TRANSLATOR_MEET_INPUT='{}'", meet_in);
        eprintln!("[ENGINE] TRANSLATOR_MEET_OUTPUT='{}'", meet_out);

        Self {
            deepgram_api_key: dg_key,
            groq_api_key: groq_key,
            yandex_api_key: yandex_key,
            yandex_folder_id: yandex_folder_id,
            translation_provider: translation_provider,
            litellm_base_url: std::env::var("LITELLM_BASE_URL").unwrap_or_default(),
            litellm_api_key: std::env::var("LITELLM_API_KEY").unwrap_or_default(),
            litellm_model: std::env::var("LITELLM_MODEL").unwrap_or_else(|_| "ollama:ministral-3:3b-cloud".into()),
            stt_provider: std::env::var("STT_PROVIDER").unwrap_or_else(|_| "auto".into()),
            whisper_model: std::env::var("WHISPER_MODEL").unwrap_or_else(|_| "tiny".into()),
            local_mt_ruen: std::env::var("TRANSLATOR_LOCAL_MT_RUEN").unwrap_or_default(),
            local_mt_enru: std::env::var("TRANSLATOR_LOCAL_MT_ENRU").unwrap_or_default(),
            tts_en_model: std::env::var("TRANSLATOR_TTS_EN_MODEL")
                .unwrap_or_else(|_| format!("{}/piper-en/en_GB-alan-low.onnx", base)),
            tts_en_config: std::env::var("TRANSLATOR_TTS_EN_CONFIG")
                .unwrap_or_else(|_| format!("{}/piper-en/en_GB-alan-low.onnx.json", base)),
            tts_ru_model: std::env::var("TRANSLATOR_TTS_RU_MODEL")
                .unwrap_or_else(|_| format!("{}/piper-ru/ru_RU-denis-medium.onnx", base)),
            tts_ru_config: std::env::var("TRANSLATOR_TTS_RU_CONFIG")
                .unwrap_or_else(|_| format!("{}/piper-ru/ru_RU-denis-medium.onnx.json", base)),
            mic_device: std::env::var("TRANSLATOR_MIC_DEVICE")
                .unwrap_or_else(|_| "default".into()),
            speaker_device: std::env::var("TRANSLATOR_SPEAKER_DEVICE")
                .unwrap_or_else(|_| "default".into()),
            meet_input_device: std::env::var("TRANSLATOR_MEET_INPUT")
                .unwrap_or_else(|_| "default".into()),
            meet_output_device: std::env::var("TRANSLATOR_MEET_OUTPUT")
                .unwrap_or_else(|_| "default".into()),
            sample_rate: std::env::var("TRANSLATOR_SAMPLE_RATE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(48000),
            endpointing_ms: std::env::var("TRANSLATOR_ENDPOINTING_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(300),
            utterance_end_ms: std::env::var("TRANSLATOR_UTTERANCE_END_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(500),
            my_language: std::env::var("TRANSLATOR_MY_LANG").unwrap_or_else(|_| "ru".into()),
            their_language: std::env::var("TRANSLATOR_THEIR_LANG").unwrap_or_else(|_| "en".into()),
        }
    }
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum EngineState {
    Idle,
    Running,
    ShuttingDown,
}

pub struct Engine {
    state: EngineState,
    config: EngineConfig,
    event_tx: Sender<Event>,
    pipeline_handles: Vec<thread::JoinHandle<()>>,
    stop_flag: Option<Arc<AtomicBool>>,
    mute_outgoing: Arc<AtomicBool>,
    mute_incoming: Arc<AtomicBool>,
}

impl Engine {
    pub fn new(config: EngineConfig, event_tx: Sender<Event>) -> Self {
        Self {
            state: EngineState::Idle,
            config,
            event_tx,
            pipeline_handles: Vec::new(),
            stop_flag: None,
            mute_outgoing: Arc::new(AtomicBool::new(false)),
            mute_incoming: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_shutting_down(&self) -> bool {
        self.state == EngineState::ShuttingDown
    }

    pub fn handle_command(&mut self, cmd: crate::protocol::Command) -> Vec<Event> {
        // Instead of per-command handling, we pattern match on the command.
        match cmd {
            Command::Ping => vec![Event::Pong],

            Command::Start { pipelines } => {
                if self.state == EngineState::Running {
                    return vec![Event::Error {
                        message: "Pipelines already running. Stop first.".into(),
                    }];
                }

                // Note: list_devices() removed here — it holds WASAPI COM references
                // that conflict with AudioCapture::new() on the pipeline threads.

                match self.start_pipelines(&pipelines) {
                    Ok(()) => {
                        self.state = EngineState::Running;
                        vec![
                            Event::Log {
                                level: "info".into(),
                                message: format!("Starting pipelines: {:?}", pipelines),
                            },
                            Event::Started {
                                pipelines: pipelines.clone(),
                            },
                        ]
                    }
                    Err(e) => {
                        error!("Failed to start pipelines: {:#}", e);
                        vec![Event::Error {
                            message: format!("Failed to start pipelines: {:#}", e),
                        }]
                    }
                }
            }

            Command::Stop => {
                self.stop_pipelines();
                self.state = EngineState::Idle;
                vec![
                    Event::Log {
                        level: "info".into(),
                        message: "Pipelines stopped".into(),
                    },
                    Event::Stopped,
                ]
            }

            Command::SetConfig { key, value } => {
                self.apply_config(&key, &value);
                vec![Event::Log {
                    level: "info".into(),
                    message: format!("Config set: {} = {}", key, value),
                }]
            }

            Command::ListDevices => match audio::list_devices() {
                Ok((input, output)) => vec![Event::DeviceList { input, output }],
                Err(e) => vec![Event::Error {
                    message: format!("Failed to list devices: {:#}", e),
                }],
            },

            Command::TtsPreview { lang, voice } => {
                let models_base = std::env::var("TRANSLATOR_MODELS_DIR")
                    .unwrap_or_else(|_| "./models".into());
                let model_path = format!("{}/piper-{}/{}.onnx", models_base, lang, voice);
                let config_path = format!("{}/piper-{}/{}.onnx.json", models_base, lang, voice);
                let text = match lang.as_str() {
                    "ru" => "Привет, это тест голоса для перевода.",
                    "de" => "Hallo, dies ist ein Stimmtest.",
                    "fr" => "Bonjour, ceci est un test de voix.",
                    "es" => "Hola, esta es una prueba de voz.",
                    "it" => "Ciao, questo è un test vocale.",
                    "pt" => "Olá, este é um teste de voz.",
                    "zh" => "你好，这是语音测试。",
                    "ar" => "مرحبا، هذا اختبار صوتي.",
                    "hi" => "नमस्ते, यह एक आवाज़ परीक्षण है।",
                    "tr" => "Merhaba, bu bir ses testidir.",
                    "nl" => "Hallo, dit is een stemtest.",
                    "pl" => "Cześć, to jest test głosu.",
                    "uk" => "Привіт, це тест голосу.",
                    _ => "Hello, this is a voice preview test.",
                };

                match TtsEngine::new(&config_path, &model_path, self.config.sample_rate) {
                    Ok(mut tts) => {
                        match tts.synthesize(text) {
                            Ok(samples) => {
                                // Play through default speakers
                                let speaker = self.config.speaker_device.clone();
                                let sr = self.config.sample_rate;
                                let (tx, rx) = crossbeam_channel::bounded(4);
                                match AudioPlayback::new(&speaker, sr, rx) {
                                    Ok(playback) => {
                                        let channels = playback.channels();
                                        let stereo_samples = if channels > 1 {
                                            let mut expanded = Vec::with_capacity(samples.len() * channels as usize);
                                            for &s in &samples {
                                                for _ in 0..channels {
                                                    expanded.push(s);
                                                }
                                            }
                                            expanded
                                        } else {
                                            samples
                                        };
                                        let _ = tx.send(stereo_samples);
                                        drop(tx);
                                        // Wait for playback to finish
                                        std::thread::sleep(std::time::Duration::from_secs(3));
                                        drop(playback);
                                        vec![Event::TtsPreviewDone]
                                    }
                                    Err(e) => vec![Event::Error {
                                        message: format!("Preview playback failed: {:#}", e),
                                    }],
                                }
                            }
                            Err(e) => vec![Event::Error {
                                message: format!("Preview synthesis failed: {:#}", e),
                            }],
                        }
                    }
                    Err(e) => vec![Event::Error {
                        message: format!("Preview TTS load failed: {:#}", e),
                    }],
                }
            }

            Command::Shutdown => {
                let mut events = Vec::new();
                if self.state == EngineState::Running {
                    self.stop_pipelines();
                    events.push(Event::Stopped);
                }
                self.state = EngineState::ShuttingDown;
                events
            }
        }
    }

    fn apply_config(&mut self, key: &str, value: &serde_json::Value) {
        match key {
            "endpointing_ms" => {
                if let Some(v) = value.as_u64() {
                    self.config.endpointing_ms = v as u32;
                    info!("Updated endpointing_ms to {}", v);
                }
            }
            "mute_outgoing" => {
                let muted = value.as_bool().unwrap_or(false);
                self.mute_outgoing.store(muted, Ordering::SeqCst);
                info!("Outgoing mute: {}", muted);
            }
            "mute_incoming" => {
                let muted = value.as_bool().unwrap_or(false);
                self.mute_incoming.store(muted, Ordering::SeqCst);
                info!("Incoming mute: {}", muted);
            }
            _ => warn!("Unknown config key: {}", key),
        }
    }

    fn start_pipelines(&mut self, pipelines: &[String]) -> Result<()> {
        let stop_flag = Arc::new(AtomicBool::new(false));
        self.stop_flag = Some(stop_flag.clone());

        info!("Loading translation models...");
        let translator = Arc::new(
            TranslationEngine::new(&self.config.groq_api_key, &self.config.yandex_api_key, &self.config.yandex_folder_id, &self.config.translation_provider, &self.config.litellm_base_url, &self.config.litellm_api_key, &self.config.litellm_model, #[cfg(feature = "local-mt")] &self.config.local_mt_ruen, #[cfg(feature = "local-mt")] &self.config.local_mt_enru)
            .context("Failed to initialize translation engine")?,
        );

        // TTS loading is deferred to the pipeline processor thread (lazy init).
        // This prevents 60MB ONNX model loading from blocking startup.

        info!("Spawning pipelines...");

        // Two-directional echo suppression:
        // - outgoing_suppress: set by INCOMING when its TTS plays to Jabra speakers,
        //   checked by OUTGOING to ignore Jabra mic picking up speaker audio.
        // - incoming_suppress: set by OUTGOING when its TTS plays to CABLE Input,
        //   checked by INCOMING to ignore CABLE Output picking up the TTS loop.
        let outgoing_suppress = Arc::new(AtomicBool::new(false));
        let incoming_suppress = Arc::new(AtomicBool::new(false));

        for pipeline_name in pipelines {
            match pipeline_name.as_str() {
"outgoing" => {
            let stt = DeepgramStt::new(
                self.config.deepgram_api_key.clone(),
                self.config.my_language.clone(),
                self.config.endpointing_ms,
                self.config.utterance_end_ms,
            );
            // Outgoing: captures from mic (Jabra), plays to CABLE Input (browser mic)
            let playback_dev = if self.config.meet_output_device.is_empty() || self.config.meet_output_device == "default" {
                tracelog::trace("engine", "DEVICE", &format!("Outgoing playback: FALLBACK to speaker_device='{}' (meet_output='{}' empty/default)", 
                    self.config.speaker_device, self.config.meet_output_device));
                self.config.speaker_device.clone()
            } else {
                tracelog::trace("engine", "DEVICE", &format!("Outgoing playback: using meet_output_device='{}'", self.config.meet_output_device));
                self.config.meet_output_device.clone()
            };
            let handle = spawn_pipeline(
                "outgoing",
                self.config.mic_device.clone(),
                playback_dev,
                self.config.sample_rate,
                stt,
                translator.clone(),
                TranslationDirection::new(&self.config.my_language, &self.config.their_language),
                &self.config.my_language,
                &self.config.tts_en_config,
                &self.config.tts_en_model,
                self.event_tx.clone(),
                stop_flag.clone(),
                self.mute_outgoing.clone(),
                outgoing_suppress.clone(),   // checked by outgoing
                incoming_suppress.clone(),   // set by outgoing (suppresses incoming)
                false,                       // no loopback for outgoing (captures from mic)
                self.config.stt_provider.clone(),
                self.config.yandex_api_key.clone(),
                self.config.yandex_folder_id.clone(),
                self.config.my_language.clone(),
                self.config.whisper_model.clone(),
            )?;
            self.pipeline_handles.push(handle);
        }
        "incoming" => {
            info!("[incoming] Pipeline enabled");
            tracelog::trace("engine", "DEVICE", &format!("Incoming pipeline: capture='{}' (meet_input), playback='{}' (speaker)", 
                self.config.meet_input_device, self.config.speaker_device));
            let stt = DeepgramStt::new(
                self.config.deepgram_api_key.clone(),
                self.config.their_language.clone(),
                self.config.endpointing_ms,
                self.config.utterance_end_ms,
            );
            let handle = spawn_pipeline(
                "incoming",
                self.config.meet_input_device.clone(),   // CABLE Output (captures remote audio)
                self.config.speaker_device.clone(),       // Jabra speakers (user hears TTS)
                self.config.sample_rate,
                stt,
                translator.clone(),
                TranslationDirection::new(&self.config.their_language, &self.config.my_language),
                &self.config.their_language,
                &self.config.tts_ru_config,
                &self.config.tts_ru_model,
                self.event_tx.clone(),
                stop_flag.clone(),
                self.mute_incoming.clone(),
                incoming_suppress.clone(),
                outgoing_suppress.clone(),
                false,                       // CABLE Output capture
                self.config.stt_provider.clone(),
                self.config.yandex_api_key.clone(),
                self.config.yandex_folder_id.clone(),
                self.config.their_language.clone(),
                self.config.whisper_model.clone(),
            )?;
            self.pipeline_handles.push(handle);
        }
                other => warn!("Unknown pipeline: {}", other),
            }
        }

        Ok(())
    }

    fn stop_pipelines(&mut self) {
        if let Some(flag) = self.stop_flag.take() {
            flag.store(true, Ordering::SeqCst);
        }

        for handle in self.pipeline_handles.drain(..) {
            let name = handle.thread().name().unwrap_or("unnamed").to_string();
            info!("Waiting for pipeline thread '{}' to stop...", name);
            if let Err(e) = handle.join() {
                error!("Pipeline thread '{}' panicked: {:?}", name, e);
            }
        }
        info!("All pipeline threads stopped");
    }
}

// ---------------------------------------------------------------------------
// Pipeline spawning
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn spawn_pipeline(
    direction: &str,
    capture_device: String,
    playback_device: String,
    sample_rate: u32,
    stt: DeepgramStt,
    translator: Arc<TranslationEngine>,
    translate_direction: TranslationDirection,
    source_lang: &str,
    tts_config: &str,
    tts_model: &str,
    event_tx: Sender<Event>,
    stop_flag: Arc<AtomicBool>,
    mute_flag: Arc<AtomicBool>,
    my_suppress: Arc<AtomicBool>,   // checked by this pipeline (set by other)
    set_other_suppress: Arc<AtomicBool>,  // set by this pipeline (checked by other)
    use_loopback: bool,
    stt_provider: String,
    yandex_key: String,
    yandex_folder_id: String,
    stt_language: String,
    whisper_model: String,
) -> Result<thread::JoinHandle<()>> {
    let dir_name = direction.to_string();
    let src_lang = source_lang.to_string();
    let tts_cfg = tts_config.to_string();
    let tts_mod = tts_model.to_string();

    let handle = thread::Builder::new()
        .name(format!("pipeline-{}", direction))
        .spawn(move || {
            let proc_set_other = set_other_suppress.clone(); // clone for processor thread
            if let Err(e) = run_pipeline(
                &dir_name,
                &capture_device,
                &playback_device,
                sample_rate,
                stt,
                &translator,
                translate_direction,
                &src_lang,
                &tts_cfg,
                &tts_mod,
                &event_tx,
                stop_flag.clone(),
                &mute_flag,
                my_suppress,
                proc_set_other,
                use_loopback,
                &stt_provider,
                &yandex_key,
                &yandex_folder_id,
                &stt_language,
                &whisper_model,
            ) {
                error!("{} pipeline failed: {:#}", dir_name, e);
                let _ = event_tx.try_send(Event::Error {
                    message: format!("{} pipeline failed: {:#}", dir_name, e),
                });
            }
            info!("{} pipeline thread exiting", dir_name);
        })
        .context("Failed to spawn pipeline thread")?;

    Ok(handle)
}

// ---------------------------------------------------------------------------
// Core pipeline logic
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn run_pipeline(
    direction: &str,
    capture_device: &str,
    playback_device: &str,
    sample_rate: u32,
    stt: DeepgramStt,
    translator: &TranslationEngine,
    translate_direction: TranslationDirection,
    source_lang: &str,
    tts_config: &str,
    tts_model: &str,
    event_tx: &Sender<Event>,
    stop_flag: Arc<AtomicBool>,
    mute_flag: &AtomicBool,
    my_suppress: Arc<AtomicBool>,       // checked by this pipeline (set by other)
    set_other_suppress: Arc<AtomicBool>, // set by this pipeline (checked by other)
    use_loopback: bool,
    stt_provider: &str,
    yandex_key: &str,
    yandex_folder_id: &str,
    stt_language: &str,
    whisper_model: &str,
) -> Result<()> {
    info!(
        "[{}] Starting pipeline: capture='{}', playback='{}', loopback={}",
        direction, capture_device, playback_device, use_loopback
    );

    let (audio_tx, audio_rx) = bounded::<AudioChunk>(512);
    let (playback_tx, playback_rx) = bounded::<Vec<f32>>(64);
    let (proc_tx, proc_rx) = bounded::<(String, u64)>(64);

    let capture_rate: u32;
    // Keep capture alive for the duration of the pipeline loop
    #[allow(unused_assignments)]
    let mut _capture_handle: Option<AudioCapture> = None;

    if use_loopback {
        // WASAPI loopback: capture audio playing through the output device (e.g., Jabra speakers)
        // This captures the meeting app's output without the echo of our outgoing TTS
        info!("[{}] Using WASAPI loopback capture from '{}'", direction, capture_device);
        tracelog::trace(direction, "DEVICE", &format!("WASAPI loopback capture from output device '{}'", capture_device));

        let (std_tx, std_rx) = std::sync::mpsc::channel();
        let loopback_stop = stop_flag.clone();
        let loopback = crate::audio::loopback::LoopbackCapture::new(capture_device, std_tx, loopback_stop)
            .with_context(|| format!("[{}] Failed to create WASAPI loopback capture", direction))?;
        capture_rate = loopback.sample_rate();
        loopback.start()
            .with_context(|| format!("[{}] Failed to start WASAPI loopback capture", direction))?;
        std::mem::forget(loopback);

        // Bridge std::mpsc → crossbeam channel in a background thread
        let audio_tx_clone = audio_tx;
        std::thread::Builder::new()
            .name(format!("loopback-bridge-{}", direction))
            .spawn(move || {
                while let Ok(chunk) = std_rx.recv() {
                    if audio_tx_clone.try_send(chunk).is_err() {
                        debug!("[loopback-bridge] Crossbeam channel full, dropping chunk");
                    }
                }
                info!("[loopback-bridge] std::mpsc channel closed, bridge exiting");
            })
            .context("Failed to spawn loopback bridge thread")?;
    } else {
        // Small delay to allow previous WASAPI sessions to fully release
        std::thread::sleep(std::time::Duration::from_millis(500));
        let capture = AudioCapture::new(capture_device, audio_tx)
            .with_context(|| format!("[{}] Failed to create AudioCapture", direction))?;
        capture_rate = capture.sample_rate();
        capture
            .start()
            .with_context(|| format!("[{}] Failed to start capture", direction))?;
        _capture_handle = Some(capture);
    }

    let playback = AudioPlayback::new(playback_device, sample_rate, playback_rx)
        .with_context(|| format!("[{}] Failed to create AudioPlayback", direction))?;

    // Use the playback device's NATIVE rate for TTS — this matches what the hardware uses
    let playback_rate = playback.sample_rate();
    let playback_channels = playback.channels();

    tracelog::trace(direction, "DEVICE", &format!(
        "Playback: rate={}Hz channels={} (mono TTS will be expanded to {}ch)",
        playback_rate, playback_channels, playback_channels
    ));

    // Connect to Deepgram — stream at 16kHz to save bandwidth
    let stt_sample_rate = 16_000_u32;

    // Create STT session based on provider config
    let mut session = if stt_provider == "yandex" && !yandex_key.is_empty() {
        info!("[{}] Using Yandex SpeechKit STT", direction);
        UnifiedSttSession::Yandex(crate::stt::yandex_stt::YandexSttSession::new(
            &yandex_key, &yandex_folder_id, &stt_language, stt_sample_rate,
        )?)
    } else if stt_provider == "whisper" {
        info!("[{}] Using local Whisper STT (model={}, lang={})", direction, whisper_model, stt_language);
        UnifiedSttSession::Whisper(crate::stt::whisper_stt::WhisperSttSession::new(
            stt_sample_rate, &whisper_model,
        )?)
    } else if stt_provider == "deepgram" || stt_provider == "auto" {
        // Try Deepgram first
        match stt.create_session(stt_sample_rate) {
            Ok(s) => {
                info!("[{}] Using Deepgram STT", direction);
                UnifiedSttSession::Deepgram(s)
            }
            Err(e) if stt_provider == "auto" && !yandex_key.is_empty() => {
                warn!("[{}] Deepgram failed ({}), falling back to Yandex STT", direction, e);
                UnifiedSttSession::Yandex(crate::stt::yandex_stt::YandexSttSession::new(
                    &yandex_key, &yandex_folder_id, &stt_language, stt_sample_rate,
                )?)
            }
            Err(e) if stt_provider == "auto" => {
                warn!("[{}] Deepgram failed ({}), falling back to local Whisper STT", direction, e);
                UnifiedSttSession::Whisper(crate::stt::whisper_stt::WhisperSttSession::new(
                    stt_sample_rate, &whisper_model,
                )?)
            }
            Err(e) => {
                bail!("[{}] Failed to create STT session: {:#}", direction, e);
            }
        }
    } else {
        bail!("[{}] No valid STT provider configured (provider='{}')", direction, stt_provider);
    };

    playback
        .start()
        .with_context(|| format!("[{}] Failed to start playback", direction))?;

    let drained = audio_rx.try_iter().count();
    if drained > 0 {
        info!("[{}] Drained {} stale audio chunks", direction, drained);
    }

    info!("[{}] Pipeline running", direction);

    // Processor thread: translate + TTS, runs independently so audio loop is never blocked.
    let proc_translator = translator.clone();
    let proc_playback_tx = playback_tx.clone();
    let proc_event_tx = event_tx.clone();
    let proc_direction = direction.to_string();
    let proc_source_lang = source_lang.to_string();
    let proc_sample_rate = playback_rate;
    let proc_playback_channels = playback_channels;
    let _proc_tts_config = tts_config.to_string();
    let _proc_tts_model = tts_model.to_string();
    // Pre-load TTS in background.
    // Use a channel so the processor thread can pick it up when ready
    // without blocking on the first utterance.
    // A watchdog timer sends None after TTS_PRELOAD_TIMEOUT_SECS if ONNX hangs.
    const TTS_PRELOAD_TIMEOUT_SECS: u64 = 30;
    let (tts_ready_tx, tts_ready_rx) = bounded::<Option<TtsEngine>>(1);
    let tts_config_bg = tts_config.to_string();
    let tts_model_bg = tts_model.to_string();
    let proc_direction_bg = proc_direction.clone();
    let proc_event_tx_bg = proc_event_tx.clone();
    let dir_name = direction.to_string();
    let _tts_preload_handle = std::thread::Builder::new()
        .name(format!("{}-tts-preload", direction))
        .spawn(move || {
            tracelog::trace(&proc_direction_bg, "TTS", "background TTS preload started...");
            let _ = proc_event_tx_bg.try_send(Event::TtsStatus {
                direction: proc_direction_bg.clone(),
                status: "starting".into(),
                message: "background TTS preload started".into(),
            });

            // Run TTS init in a separate thread with a timeout
            let (init_tx, init_rx) = std::sync::mpsc::channel();
            let bg_config = tts_config_bg.clone();
            let bg_model = tts_model_bg.clone();
            let bg_dir = proc_direction_bg.clone();
            let _init_thread = std::thread::Builder::new()
                .name(format!("{}-tts-init-worker", dir_name))
                .spawn(move || {
                    tracelog::trace(&bg_dir, "TTS", "TTS init worker started");
                    // Initialize ONNX Runtime on this thread (not on main thread)
                    // to avoid COM interference with WASAPI audio capture.
                    let _ = ort::init().commit();
                    tracelog::trace(&bg_dir, "TTS", "ONNX Runtime init done");
                    let result = TtsEngine::new(&bg_config, &bg_model, proc_sample_rate);
                    let _ = init_tx.send(result);
                });

            // Wait up to TTS_PRELOAD_TIMEOUT_SECS - 5 for the init to complete
            let init_timeout = std::time::Duration::from_secs(TTS_PRELOAD_TIMEOUT_SECS.saturating_sub(5).max(10));
            match init_rx.recv_timeout(init_timeout) {
                Ok(Ok(engine)) => {
                    tracelog::trace(&proc_direction_bg, "TTS", "background TTS preload OK ✓");
                    let _ = proc_event_tx_bg.try_send(Event::TtsStatus {
                        direction: proc_direction_bg.clone(),
                        status: "ok".into(),
                        message: "TTS engine ready ✓".into(),
                    });
                    let _ = tts_ready_tx.send(Some(engine));
                }
                Ok(Err(e)) => {
                    tracelog::trace(&proc_direction_bg, "ERROR", &format!("TTS preload failed: {} — continuing without TTS", e));
                    let _ = proc_event_tx_bg.try_send(Event::TtsStatus {
                        direction: proc_direction_bg.clone(),
                        status: "fail".into(),
                        message: format!("{}", e),
                    });
                    let _ = tts_ready_tx.send(None);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    tracelog::trace(&proc_direction_bg, "ERROR", "TTS preload thread crashed");
                    let _ = proc_event_tx_bg.try_send(Event::TtsStatus {
                        direction: proc_direction_bg.clone(),
                        status: "fail".into(),
                        message: "TTS preload thread crashed".into(),
                    });
                    let _ = tts_ready_tx.send(None);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    tracelog::trace(&proc_direction_bg, "ERROR", &format!("TTS preload timed out after {}s — continuing without TTS", TTS_PRELOAD_TIMEOUT_SECS));
                    let _ = proc_event_tx_bg.try_send(Event::TtsStatus {
                        direction: proc_direction_bg.clone(),
                        status: "fail".into(),
                        message: format!("TTS preload timed out after {}s", TTS_PRELOAD_TIMEOUT_SECS),
                    });
                    let _ = tts_ready_tx.send(None);
                }
            }
        });

    let mut tts: Option<TtsEngine> = None;
    let mut segmenter = crate::pipeline::segmenter::TextSegmenter::new();
    let job_queue = crate::pipeline::job_queue::JobQueue::new();
    let mut utterance_start: Option<Instant> = None;

    let _proc_echo = set_other_suppress.clone();
    let _proc_handle = std::thread::spawn(move || {
        while let Ok((text, stt_ms)) = proc_rx.recv() {
            // Check if TTS has finished loading (non-blocking).
            if let Ok(Some(engine)) = tts_ready_rx.try_recv() {
                tracelog::trace(&proc_direction, "TTS", "TTS engine ready, switching to full pipeline ✓");
                tts = Some(engine);
            } else if let Ok(None) = tts_ready_rx.try_recv() {
                tracelog::trace(&proc_direction, "TTS", "TTS engine failed, continuing without TTS");
                tts = None;
            }

            // Segmenter: extract only the incremental delta from stable partials
            // to avoid retranslating the same prefix. Finals reset the segmenter.
            let text_to_process = if let Some(delta) = segmenter.next_segment(&text) {
                tracelog::trace(&proc_direction, "SEGMENT", &format!("delta '{}' (full: '{}')", delta, text));
                delta
            } else {
                tracelog::trace(&proc_direction, "SEGMENT", &format!("no new segment in '{}'", text));
                // Still emit the transcript event for the UI, but skip translation
                let _ = proc_event_tx.try_send(Event::Transcript {
                    direction: proc_direction.clone(),
                    text: text.clone(),
                    lang: proc_source_lang.clone(),
                    stt_ms,
                });
                continue;
            };

            let job_id = job_queue.next_job();
            tracelog::trace(&proc_direction, "PROCESSOR", &format!("JOB #{} text='{}' stt={}ms", job_id, text_to_process, stt_ms));

            // Track utterance start for time-to-first-audio metric
            if utterance_start.is_none() {
                utterance_start = Some(Instant::now());
            }

            let _audio_len = process_utterance(
                &proc_direction,
                &text_to_process,
                stt_ms,
                &proc_translator,
                &translate_direction,
                &proc_source_lang,
                &mut tts,
                proc_sample_rate,
                proc_playback_channels,
                &proc_playback_tx,
                &proc_event_tx,
            );

            // Emit time-to-first-audio when audio is first produced for this utterance
            if _audio_len > 0 {
                if let Some(start) = utterance_start.take() {
                    let first_audio_ms = start.elapsed().as_millis() as u64;
                    tracelog::trace(&proc_direction, "METRICS", &format!("first_audio={}ms", first_audio_ms));
                    let _ = proc_event_tx.try_send(Event::Log {
                        level: "info".into(),
                        message: format!("[{}] first_audio={}ms", proc_direction, first_audio_ms),
                    });
                }
            }

            // If this job is stale (newer job already completed), log the drop
            if job_queue.is_stale(job_id) && _audio_len > 0 {
                tracelog::trace(&proc_direction, "PROCESSOR", &format!("stale job #{} dropped (newer job already committed)", job_id));
            } else {
                job_queue.try_commit(job_id);
            }

            // Echo suppression: only outgoing suppresses incoming (TTS goes to CABLE, not Jabra)
            if _audio_len > 0 && proc_direction == "outgoing" {
                _proc_echo.store(true, Ordering::SeqCst);
                let es = _proc_echo.clone();
                let dur_ms = (_audio_len as f32 / proc_sample_rate as f32 * 1000.0) as u64 + 2000;
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(dur_ms));
                    es.store(false, Ordering::SeqCst);
                });
            }
        }
    });

    info!("[{}] Capture rate: {}Hz, STT rate: {}Hz", direction, capture_rate, stt_sample_rate);

    let mut need_reconnect = false;
    let mut reconnect_attempts: u32 = 0;
    let mut deepgram_drop_count: u32 = 0;
    const MAX_DEEPGRAM_DROPS: u32 = 3;
    const MAX_BACKOFF_MS: u64 = 30000;
    let mut loop_count: u64 = 0;
    let diag_interval = 100u64;
    let mut last_audio_sent = Instant::now();
    loop {
        if stop_flag.load(Ordering::SeqCst) {
            info!("[{}] Stop flag set, exiting", direction);
            break;
        }

        if need_reconnect {
            reconnect_attempts += 1;
            let backoff_ms = std::cmp::min(
                2000u64 * 2u64.pow(std::cmp::min(reconnect_attempts.saturating_sub(1), 16)),
                MAX_BACKOFF_MS,
            );
            info!("[{}] Reconnecting STT (attempt #{}, {}ms backoff, drop_count={})...", direction, reconnect_attempts, backoff_ms, deepgram_drop_count);
            session.close();
            std::thread::sleep(Duration::from_millis(backoff_ms));

            // Auto mode: after MAX_DEEPGRAM_DROPS disconnections, switch to Yandex, then Whisper
            if deepgram_drop_count >= MAX_DEEPGRAM_DROPS && stt_provider == "auto" {
                if !yandex_key.is_empty() && !matches!(session, UnifiedSttSession::Yandex(_)) {
                    warn!("[{}] Switching to Yandex STT after {} Deepgram drops", direction, deepgram_drop_count);
                    if let Ok(ys) = crate::stt::yandex_stt::YandexSttSession::new(
                        &yandex_key, &yandex_folder_id, &stt_language, stt_sample_rate,
                    ) {
                        session = UnifiedSttSession::Yandex(ys);
                        need_reconnect = false;
                        reconnect_attempts = 0;
                        info!("[{}] Switched to Yandex STT ✓", direction);
                        continue;
                    }
                }
                // If Yandex failed or not available, use local Whisper
                if !matches!(session, UnifiedSttSession::Whisper(_)) {
                    warn!("[{}] Switching to local Whisper STT", direction);
                    match crate::stt::whisper_stt::WhisperSttSession::new(stt_sample_rate, &whisper_model) {
                        Ok(ws) => {
                            session = UnifiedSttSession::Whisper(ws);
                            need_reconnect = false;
                            reconnect_attempts = 0;
                            info!("[{}] Switched to local Whisper STT ✓", direction);
                            continue;
                        }
                        Err(e) => {
                            error!("[{}] Whisper init failed: {:#}", direction, e);
                            continue;
                        }
                    }
                }
            }

            // Try reconnecting to Deepgram
            match stt.create_session(stt_sample_rate) {
                Ok(new_session) => {
                    session = UnifiedSttSession::Deepgram(new_session);
                    need_reconnect = false;
                    reconnect_attempts = 0;
                    info!("[{}] Deepgram reconnected (drop_count={})", direction, deepgram_drop_count);
                }
                Err(e) => {
                    error!("[{}] Deepgram reconnect failed: {:#}", direction, e);
                    continue;
                }
            }
        }

        // Send ALL available audio chunks to Deepgram
        let mut chunks_sent = 0usize;
        let mut total_chunks_with_audio = 0usize;
        for chunk in audio_rx.try_iter().take(10) {
            if mute_flag.load(Ordering::Relaxed) {
                continue;
            }
            let samples_16k = resample(&chunk.samples, capture_rate, stt_sample_rate);
            let rms = (samples_16k.iter().map(|s| s * s).sum::<f32>() / samples_16k.len().max(1) as f32).sqrt();
            if rms > 0.01 { total_chunks_with_audio += 1; }
            if chunks_sent < 3 || (loop_count % 500 == 0 && rms > 0.01) {
                tracelog::trace(direction, "CAPTURE", &format!("chunk {} samples, rms={:.6}, capture_rate={}Hz→{}Hz", samples_16k.len(), rms, capture_rate, stt_sample_rate));
            }
            if let Err(e) = session.send_audio(&samples_16k) {
                warn!("[{}] STT send error: {:#}", direction, e);
                tracelog::trace(direction, "ERROR", &format!("STT send FAILED: {}", e));
                deepgram_drop_count += 1;
                need_reconnect = true;
                break;
            }
            chunks_sent += 1;
        }
        if chunks_sent > 0 {
            last_audio_sent = Instant::now();
            if loop_count % 500 == 0 || (loop_count < 10) {
                tracelog::trace(direction, "CAPTURE", &format!("batch sent: {} chunks ({} with speech), total_loop={}", chunks_sent, total_chunks_with_audio, loop_count));
            }
        }
        // Send silence keepalive every ~8s when no audio was captured, to prevent
        // Deepgram server-side idle timeout (~12s when no data arrives).
        if chunks_sent == 0 && !need_reconnect {
            if last_audio_sent.elapsed() >= Duration::from_secs(8) {
                let silence_samples = (stt_sample_rate as u64 * 8 / 1000) as usize;
                let silence: Vec<f32> = vec![0.0; silence_samples];
                if let Err(e) = session.send_audio(&silence) {
                    warn!("[{}] STT keepalive send error: {:#}", direction, e);
                    deepgram_drop_count += 1;
                    need_reconnect = true;
                }
                let _ = session.flush_pending();
                last_audio_sent = Instant::now();
            }
        }

        // Flush any pending buffered audio (from previous WouldBlock)
        let _ = session.flush_pending();

        loop_count += 1;
        if loop_count % diag_interval == 0 {
            let _ = event_tx.try_send(Event::Log {
                level: "debug".into(),
                message: format!("[{}] Loop #{} alive", direction, loop_count),
            });
        }

        // Non-blocking poll — returns immediately on WouldBlock
        match session.poll_transcript() {
            Ok(Some(result)) => {
                match result.kind {
                    crate::stt::SttResultKind::Partial => {
                        tracelog::trace(direction, "STT", &format!("PARTIAL text='{}'", result.text));
                        let _ = event_tx.try_send(Event::PartialTranscript {
                            direction: direction.to_string(),
                            text: result.text,
                            lang: source_lang.to_string(),
                            stt_ms: result.stt_latency_ms,
                        });
                    }
                    crate::stt::SttResultKind::StablePartial => {
                        tracelog::trace(direction, "STT", &format!("STABLE_PARTIAL stt={}ms text='{}'", result.stt_latency_ms, result.text));
                        let _ = event_tx.try_send(Event::StablePartialTranscript {
                            direction: direction.to_string(),
                            text: result.text.clone(),
                            lang: source_lang.to_string(),
                            stt_ms: result.stt_latency_ms,
                        });
                        // Stable partials also go to the processor for early translation
                        if let Err(e) = proc_tx.try_send((result.text, result.stt_latency_ms)) {
                            warn!("[{}] Processor channel full, dropping stable partial: {}", direction, e);
                        }
                    }
                    crate::stt::SttResultKind::Final => {
                        tracelog::trace(direction, "STT", &format!("FINAL stt={}ms text='{}'", result.stt_latency_ms, result.text));
                        if let Err(e) = proc_tx.try_send((result.text.clone(), result.stt_latency_ms)) {
                            warn!("[{}] Processor channel full, dropping transcript: {}", direction, e);
                        } else {
                            tracelog::trace(direction, "STT", &format!("→ processor: '{}'", result.text));
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                error!("[{}] STT error: {:#}", direction, e);
                tracelog::trace(direction, "ERROR", &format!("STT error: {}", e));
                let _ = event_tx.try_send(Event::Error {
                    message: format!("[{}] STT error: {:#}", direction, e),
                });
                deepgram_drop_count += 1;
                need_reconnect = true;
            }
        }

        // Brief sleep to avoid busy-waiting on non-blocking reads
        if chunks_sent == 0 {
            std::thread::sleep(Duration::from_millis(2));
        }

    }

    session.close();
    let _ = playback.stop();
    drop(playback_tx);

    info!("[{}] Pipeline stopped cleanly", direction);
    Ok(())
}

// ---------------------------------------------------------------------------
// Utterance processing: transcript -> translate -> TTS -> playback
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn process_utterance(
    direction: &str,
    text: &str,
    stt_ms: u64,
    translator: &TranslationEngine,
    translate_direction: &TranslationDirection,
    source_lang: &str,
    tts: &mut Option<TtsEngine>,
    sample_rate: u32,
    playback_channels: u16,
    playback_tx: &Sender<Vec<f32>>,
    event_tx: &Sender<Event>,
) -> usize {
    tracelog::trace(direction, "EVENT", &format!("→ Elixir: transcript '{}' (lang={})", text, source_lang));

    let _ = event_tx.try_send(Event::Transcript {
        direction: direction.to_string(),
        text: text.to_string(),
        lang: source_lang.to_string(),
        stt_ms,
    });
    tracelog::trace(direction, "EVENT", "transcript event sent to Elixir ✓");

    // === TRANSLATION ===
    tracelog::trace(direction, "TRANSLATE", &format!("translating '{}' ...", text));
    let translate_start = Instant::now();

    // Run translation in a separate thread with a hard timeout via channel.
    // Using join() directly has no timeout and blocks the processor forever if HTTP hangs.
    const TRANSLATE_TIMEOUT_SECS: u64 = 12;
    let translate_text = text.to_string();
    let translate_dir = translate_direction.clone();
    let translator_clone = translator.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new()
        .name(format!("{}-translate-worker", direction))
        .spawn(move || {
            let result = translator_clone.translate(&translate_text, &translate_dir);
            let _ = tx.send(result);
        });

    let (translated, provider_used) = match rx.recv_timeout(Duration::from_secs(TRANSLATE_TIMEOUT_SECS)) {
        Ok(Ok((t, p))) => {
            let ms = translate_start.elapsed().as_millis() as u64;
            tracelog::trace(direction, "TRANSLATE", &format!("OK {}ms provider={} result='{}'", ms, p, t));
            (t, p)
        }
        Ok(Err(e)) => {
            let ms = translate_start.elapsed().as_millis() as u64;
            error!("[{}] Translation error: {:#}", direction, e);
            tracelog::trace(direction, "ERROR", &format!("TRANSLATION_FAILED {}ms: {}", ms, e));
            let _ = event_tx.try_send(Event::Translation {
                direction: direction.to_string(),
                text: format!("ERROR: {}", e),
                translate_ms: ms,
            });
            return 0;
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            let ms = translate_start.elapsed().as_millis() as u64;
            error!("[{}] Translation thread died (panicked?) after {}ms", direction, ms);
            tracelog::trace(direction, "ERROR", &format!("TRANSLATION_THREAD_DIED {}ms", ms));
            let _ = event_tx.try_send(Event::Translation {
                direction: direction.to_string(),
                text: "ERROR: translator crashed".to_string(),
                translate_ms: ms,
            });
            return 0;
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            let ms = translate_start.elapsed().as_millis() as u64;
            error!("[{}] Translation timed out after {}ms (limit {}s)", direction, ms, TRANSLATE_TIMEOUT_SECS);
            tracelog::trace(direction, "ERROR", &format!("TRANSLATION_TIMEOUT {}ms — skipping", ms));
            let _ = event_tx.try_send(Event::Translation {
                direction: direction.to_string(),
                text: format!("TIMEOUT after {}ms", ms),
                translate_ms: ms,
            });
            return 0;
        }
    };

    let translate_ms = translate_start.elapsed().as_millis() as u64;

    let _ = event_tx.try_send(Event::Translation {
        direction: direction.to_string(),
        text: translated.clone(),
        translate_ms,
    });
    tracelog::trace(direction, "EVENT", &format!("→ Elixir: translation '{}'", translated));

    if translated.trim().is_empty() {
        tracelog::trace(direction, "TRANSLATE", "EMPTY result — skipping TTS");
        return 0;
    }

    // === TTS ===
    tracelog::trace(direction, "TTS", &format!("synthesizing '{}' ...", translated));
    let tts_start = Instant::now();
    let audio = if let Some(tts_engine) = tts.as_mut() {
        match tts_engine.synthesize(&translated) {
            Ok(samples) => {
                let ms = tts_start.elapsed().as_millis() as u64;
                tracelog::trace(direction, "TTS", &format!("OK {}ms samples={}", ms, samples.len()));
                samples
            }
            Err(e) => {
                let ms = tts_start.elapsed().as_millis() as u64;
                error!("[{}] TTS error: {:#}", direction, e);
                tracelog::trace(direction, "ERROR", &format!("TTS_FAILED {}ms: {}", ms, e));
                let _ = event_tx.try_send(Event::Error {
                    message: format!("[{}] TTS failed: {:#}", direction, e),
                });
                return 0;
            }
        }
    } else {
        tracelog::trace(direction, "TTS", "engine disabled — skipping synthesis");
        Vec::new()
    };
    let tts_ms = tts_start.elapsed().as_millis() as u64;

    let audio_len = audio.len();

    // === PLAYBACK ===
    if !audio.is_empty() {
        tracelog::trace(direction, "PLAYBACK", &format!("queuing {} samples ({}Hz, {}ch) to device", audio.len(), sample_rate, playback_channels));
        // Downsample to 16kHz for browser monitor (good quality, ~40KB per phrase)
        let monitor_rate = 16000u32;
        let monitor_samples = resample(&audio, sample_rate, monitor_rate);
        let mut pcm_bytes = Vec::with_capacity(monitor_samples.len() * 2);
        for &s in &monitor_samples {
            let i = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            pcm_bytes.extend_from_slice(&i.to_le_bytes());
        }
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&pcm_bytes);
        let _ = event_tx.try_send(Event::TtsAudio {
            direction: direction.to_string(),
            sample_rate: monitor_rate,
            audio_b64: b64,
        });
        tracelog::trace(direction, "EVENT", "→ Elixir: tts_audio sent");

        // TTS generates mono audio. If playback device is stereo (2ch),
        // duplicate each sample so the ring buffer has the right frame count.
        // Without this, stereo devices play mono at 2x speed.
        let playback_channels = playback_channels;
        let stereo_audio = if playback_channels > 1 {
            let mut expanded = Vec::with_capacity(audio.len() * playback_channels as usize);
            for &s in &audio {
                for _ in 0..playback_channels {
                    expanded.push(s);
                }
            }
            expanded
        } else {
            audio
        };

        if let Err(e) = playback_tx.try_send(stereo_audio) {
            warn!("[{}] Playback channel full or disconnected: {}", direction, e);
            tracelog::trace(direction, "PLAYBACK", &format!("CHANNEL_FULL: {}", e));
        } else {
            tracelog::trace(direction, "PLAYBACK", "queued for speaker output ✓");
        }
    } else {
        tracelog::trace(direction, "PLAYBACK", "no audio to play (TTS disabled or empty)");
    }

    let _ = event_tx.try_send(Event::Metrics {
        direction: direction.to_string(),
        stt_ms,
        translate_ms,
        tts_ms,
        provider_used: Some(provider_used.clone()),
        time_to_first_audio_ms: None,
    });
    tracelog::trace(direction, "METRICS", &format!("stt={}ms translate={}ms tts={}ms total={}ms provider={}",
        stt_ms, translate_ms, tts_ms, stt_ms + translate_ms + tts_ms, provider_used));

    audio_len
}

// ---------------------------------------------------------------------------
// Audio utility
// ---------------------------------------------------------------------------

/// Resample audio from `from_rate` to `to_rate` using linear interpolation.
/// Handles arbitrary rate ratios (e.g. 24000→16000, 48000→16000).
fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (samples.len() as f64 / ratio) as usize;
    (0..output_len)
        .map(|i| {
            let src = i as f64 * ratio;
            let idx = src as usize;
            let frac = src - idx as f64;
            if idx + 1 < samples.len() {
                samples[idx] * (1.0 - frac as f32) + samples[idx + 1] * frac as f32
            } else {
                samples[idx.min(samples.len() - 1)]
            }
        })
        .collect()
}
