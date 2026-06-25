use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use crate::tracelog;

pub struct WhisperSttSession {
    speech_buffer: Vec<f32>,
    sample_rate: u32,
    model_name: String,
    language: String,
    raw_rms_sum: f64,
    raw_rms_count: u64,
    speech_active: bool,
    silence_chunks: u32,
}

const BUFFER_SECS: f32 = 2.0;
const SPEECH_THRESHOLD: f32 = 0.001;
const SILENCE_TIMEOUT_CHUNKS: u32 = 15;

impl WhisperSttSession {
    pub fn new(sample_rate: u32, model_name: &str, language: &str) -> Result<Self> {
        tracelog::trace("stt", "STT", &format!(
            "Local Whisper STT initialized (model={}, lang={}, rate={}Hz)", model_name, language, sample_rate
        ));
        Ok(Self {
            speech_buffer: Vec::new(),
            sample_rate,
            model_name: model_name.to_string(),
            language: language.to_string(),
            raw_rms_sum: 0.0,
            raw_rms_count: 0,
            speech_active: false,
            silence_chunks: 0,
        })
    }

    pub fn send_audio(&mut self, samples: &[f32], raw_rms: f32) -> Result<()> {
        if raw_rms >= SPEECH_THRESHOLD {
            if !self.speech_active {
                self.speech_active = true;
                self.silence_chunks = 0;
                self.raw_rms_sum = 0.0;
                self.raw_rms_count = 0;
                tracelog::trace("stt", "STT", &format!(
                    "VAD: speech started (raw_rms={:.4})", raw_rms
                ));
            }
            self.silence_chunks = 0;
            self.raw_rms_sum += raw_rms as f64;
            self.raw_rms_count += 1;
            self.speech_buffer.extend_from_slice(samples);
        } else if self.speech_active {
            self.silence_chunks += 1;
            if self.silence_chunks >= SILENCE_TIMEOUT_CHUNKS {
                let speech_duration = self.speech_buffer.len() as f32 / self.sample_rate as f32;
                tracelog::trace("stt", "STT", &format!(
                    "VAD: speech ended after {:.2}s ({} samples, silence_chunks={})",
                    speech_duration, self.speech_buffer.len(), self.silence_chunks
                ));
                self.speech_active = false;
                self.silence_chunks = 0;
            }
        }

        Ok(())
    }

    pub fn poll_transcript(&mut self) -> Result<Option<super::SttResult>> {
        let speech_samples = self.speech_buffer.len();
        let speech_duration = speech_samples as f32 / self.sample_rate as f32;
        let has_speech = speech_samples > 0 && speech_duration >= 0.1;

        let should_process = if has_speech && !self.speech_active {
            true
        } else if has_speech && speech_duration >= BUFFER_SECS {
            true
        } else {
            false
        };

        if !should_process {
            return Ok(None);
        }

        let audio = std::mem::take(&mut self.speech_buffer);

        let avg_raw_rms = if self.raw_rms_count > 0 {
            (self.raw_rms_sum / self.raw_rms_count as f64) as f32
        } else {
            0.0
        };
        self.raw_rms_sum = 0.0;
        self.raw_rms_count = 0;

        if avg_raw_rms < SPEECH_THRESHOLD {
            return Ok(None);
        }

        let start = std::time::Instant::now();

        // Save audio to WAV file
        let wav_path = std::env::temp_dir().join(format!("whisper_{}.wav", std::process::id()));
        {
            let mut f = std::fs::File::create(&wav_path)
                .context("Failed to create temp WAV file")?;
            let num_samples = audio.len() as u32;
            let data_size = num_samples * 2;
            let header = [
                b'R', b'I', b'F', b'F',
                (36 + data_size) as u8, ((36 + data_size) >> 8) as u8, ((36 + data_size) >> 16) as u8, ((36 + data_size) >> 24) as u8,
                b'W', b'A', b'V', b'E',
                b'f', b'm', b't', b' ',
                16u8, 0, 0, 0,
                1u8, 0,
                1u8, 0,
                (self.sample_rate) as u8, (self.sample_rate >> 8) as u8, 0, 0,
                (self.sample_rate * 2) as u8, ((self.sample_rate * 2) >> 8) as u8, 0, 0,
                2u8, 0,
                16u8, 0,
                b'd', b'a', b't', b'a',
                data_size as u8, (data_size >> 8) as u8, (data_size >> 16) as u8, (data_size >> 24) as u8,
            ];
            f.write_all(&header)?;
            for &sample in &audio {
                let s16 = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
                f.write_all(&s16.to_le_bytes())?;
            }
            f.sync_all()?;
        }

        // Run Whisper — use wrapper script (pythonw, no visible window)
        let result_path = std::env::temp_dir().join(format!("whisper_result_{}.json", std::process::id()));

        let script_path = std::env::current_dir().unwrap_or_default().join("whisper_run.py");
        let script_str = script_path.to_string_lossy().replace('\\', "\\\\");
        let model = &self.model_name;
        let wav_str = wav_path.to_string_lossy().replace('\\', "\\\\");
        let res_str = result_path.to_string_lossy().replace('\\', "\\\\");

        let mut cmd = Command::new(r"C:\Program Files\Python312\python.exe");
        cmd.arg(&script_str).arg(model).arg(&wav_str).arg(&res_str).arg(&self.language)
            .env("WHISPER_MODEL", &self.model_name)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let _ = cmd.output().context("Failed to run Whisper")?;

        let latency_ms = start.elapsed().as_millis() as u64;

        // Read result from file
        let stdout = std::fs::read_to_string(&result_path).unwrap_or_default();

        // Clean up
        let _ = std::fs::remove_file(&wav_path);
        let _ = std::fs::remove_file(&result_path);

        if stdout.trim().is_empty() {
            return Ok(None);
        }

        let result: WhisperResponse = match serde_json::from_str(&stdout) {
            Ok(r) => r,
            Err(e) => {
                tracelog::trace("stt", "ERROR", &format!("Whisper parse error: {}", e));
                return Err(anyhow::anyhow!("Whisper parse error: {}", e));
            }
        };

        if !result.text.trim().is_empty() {
            tracelog::trace("stt", "STT", &format!(
                "WHISPER stt={}ms lang={} text='{}'",
                latency_ms, result.language, result.text
            ));
            Ok(Some(super::SttResult {
                text: result.text,
                stt_latency_ms: latency_ms,
            }))
        } else {
            Ok(None)
        }
    }
}

use serde::Deserialize;

#[derive(Deserialize)]
struct WhisperResponse {
    text: String,
    #[allow(dead_code)]
    language: String,
    #[allow(dead_code)]
    duration: f32,
}
