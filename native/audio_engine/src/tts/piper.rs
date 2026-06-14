use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use log::debug;
use ort::session::Session;
use ort::value::Value;
use serde::Deserialize;

use crate::tracelog;

// ---------------------------------------------------------------------------
// Piper TTS — wrapper around the Piper ONNX VITS model + espeak-ng
// ---------------------------------------------------------------------------

const BOS: &str = "^"; // beginning of sentence
const EOS: &str = "$"; // end of sentence
const PAD: &str = "_"; // padding between phonemes

/// Piper model configuration parsed from the `.onnx.json` file.
#[derive(Deserialize)]
struct PiperConfig {
    audio: PiperAudio,
    espeak: PiperEspeak,
    inference: PiperInference,
    phoneme_id_map: HashMap<String, Vec<i64>>,
    num_speakers: u32,
    #[allow(dead_code)]
    speaker_id_map: HashMap<String, i64>,
}

#[derive(Deserialize)]
struct PiperAudio {
    sample_rate: u32,
}

#[derive(Deserialize)]
struct PiperEspeak {
    voice: String,
}

#[derive(Deserialize)]
struct PiperInference {
    noise_scale: f32,
    length_scale: f32,
    noise_w: f32,
}

/// Internal state for a single Piper TTS session.
pub struct PiperTts {
    session: Session,
    config: PiperConfig,
    output_sample_rate: u32,
    espeak_binary: String,
}

impl PiperTts {
    /// Create a new Piper TTS instance.
    ///
    /// `config_path` — path to the `.onnx.json` config file.
    /// `model_path`  — path to the `.onnx` model file.
    /// `output_sample_rate` — target sample rate (e.g. 48000).
    pub fn new(config_path: &str, model_path: &str, output_sample_rate: u32) -> Result<Self> {
        tracelog::trace("tts", "TTS", &format!("Step 1: reading config from {}", config_path));
        let config_str = std::fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read Piper config: {}", config_path))?;
        tracelog::trace("tts", "TTS", &format!("Step 1: config read OK ({} bytes)", config_str.len()));

        tracelog::trace("tts", "TTS", "Step 2: parsing JSON config");
        let config: PiperConfig = serde_json::from_str(&config_str)
            .with_context(|| format!("Failed to parse Piper config: {}", config_path))?;
        tracelog::trace("tts", "TTS", &format!("Step 2: OK — voice={}, rate={}, phonemes={}",
            config.espeak.voice, config.audio.sample_rate, config.phoneme_id_map.len()));

        tracelog::trace("tts", "TTS", &format!("Step 3: checking model file: {}", model_path));
        let model = Path::new(model_path);
        anyhow::ensure!(model.exists(), "Piper model not found: {}", model_path);
        let model_size = std::fs::metadata(model_path).map(|m| m.len()).unwrap_or(0);
        tracelog::trace("tts", "TTS", &format!("Step 3: model exists ({} bytes)", model_size));

        tracelog::trace("tts", "TTS", "Step 4: creating ONNX Session builder...");
        eprintln!("[TTS] Step 4: creating ONNX Session builder...");
        let mut builder = Session::builder()
            .context("Failed to create ONNX session builder")?;
        tracelog::trace("tts", "TTS", "Step 4: builder created OK");
        eprintln!("[TTS] Step 4: builder created OK");

        #[cfg(feature = "directml")]
        {
            tracelog::trace("tts", "TTS", "Step 4b: registering DirectML GPU provider...");
            eprintln!("[TTS] Step 4b: registering DirectML GPU provider...");
            if let Err(e) = builder.with_execution_providers([ort::execution_providers::DirectMLExecutionProvider::default().build()]) {
                tracelog::trace("tts", "TTS", &format!("DirectML init failed: {} — falling back to CPU", e));
                eprintln!("[TTS] DirectML init failed: {} — falling back to CPU", e);
            } else {
                tracelog::trace("tts", "TTS", "DirectML GPU provider registered ✓");
                eprintln!("[TTS] DirectML GPU provider registered ✓");
            }
        }
        #[cfg(not(feature = "directml"))]
        {
            tracelog::trace("tts", "TTS", "DirectML feature not enabled, using CPU");
        }

        tracelog::trace("tts", "TTS", "Step 5: loading ONNX model...");
        eprintln!("[TTS] Step 5: loading model from: {}", model_path);
        let session = builder.commit_from_file(model_path)
            .with_context(|| format!("Failed to load Piper ONNX model: {}", model_path))?;
        tracelog::trace("tts", "TTS", "Step 5: ONNX loaded OK ✓");
        eprintln!("[TTS] Step 5: ONNX loaded OK ✓");

        tracelog::trace("tts", "TTS", "Step 6: finding espeak-ng binary...");
        let espeak_binary = find_espeak_ng()?;
        tracelog::trace("tts", "TTS", &format!("Step 6: espeak-ng={} ✓", espeak_binary));

        Ok(Self { session, config, output_sample_rate, espeak_binary })
    }

    /// Synthesize text to f32 audio samples at `output_sample_rate`.
    pub fn synthesize(&mut self, text: &str) -> Result<Vec<f32>> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }

        let _start = std::time::Instant::now();
        tracelog::trace("tts", "TTS", &format!("synthesize: '{}'", text));

        // Step 1: Text -> IPA phonemes via espeak-ng
        tracelog::trace("tts", "TTS", &format!("Step 1: phonemize via {}", self.espeak_binary));
        let phonemes = self.phonemize(text)?;
        tracelog::trace("tts", "TTS", &format!("Step 1: phonemes='{}'", phonemes));

        // Step 2: Phonemes -> phoneme IDs
        let phoneme_ids = self.phonemes_to_ids(&phonemes);
        tracelog::trace("tts", "TTS", &format!("Step 2: {} phoneme IDs", phoneme_ids.len()));

        if phoneme_ids.is_empty() {
            tracelog::trace("tts", "TTS", "Step 2: EMPTY IDs — returning silence");
            return Ok(Vec::new());
        }

        // Step 3: Run ONNX inference
        tracelog::trace("tts", "TTS", "Step 3: ONNX inference...");
        let raw_audio = self.infer(&phoneme_ids)?;
        tracelog::trace("tts", "TTS", &format!("Step 3: got {} samples", raw_audio.len()));

        // Step 4: Resample to output rate if necessary
        if self.config.audio.sample_rate != self.output_sample_rate {
            let resampled = resample(&raw_audio, self.config.audio.sample_rate, self.output_sample_rate)?;
            tracelog::trace("tts", "TTS", &format!("Step 4: resampled {} -> {} samples", raw_audio.len(), resampled.len()));
            Ok(resampled)
        } else {
            tracelog::trace("tts", "TTS", &format!("Step 4: no resample, {} samples @ {}Hz", raw_audio.len(), self.output_sample_rate));
            Ok(raw_audio)
        }
    }

    /// Call espeak-ng to convert text to IPA phonemes.
    fn phonemize(&self, text: &str) -> Result<String> {
        let output = Command::new(&self.espeak_binary)
            .args(["-q", "--ipa=2", "-v", &self.config.espeak.voice, text])
            .output()
            .context("Failed to run espeak-ng for phonemization")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("espeak-ng failed: {}", stderr);
        }

        let phonemes = String::from_utf8(output.stdout)
            .context("espeak-ng output is not valid UTF-8")?
            .trim()
            .to_string();

        Ok(phonemes)
    }

    /// Convert IPA phoneme string to a sequence of phoneme IDs using the model's phoneme_id_map.
    /// Follows Piper convention: BOS + (phoneme + PAD)* + EOS
    fn phonemes_to_ids(&self, phonemes: &str) -> Vec<i64> {
        let map = &self.config.phoneme_id_map;
        let mut ids: Vec<i64> = Vec::new();

        if let Some(bos_ids) = map.get(BOS) { ids.extend(bos_ids); }

        for ch in phonemes.chars() {
            let key = ch.to_string();
            if let Some(ph_ids) = map.get(&key) {
                ids.extend(ph_ids);
            } else {
                debug!("Unknown phoneme in map: '{}' (U+{:04X})", ch, ch as u32);
                continue;
            }
            if let Some(pad_ids) = map.get(PAD) { ids.extend(pad_ids); }
        }

        if let Some(eos_ids) = map.get(EOS) { ids.extend(eos_ids); }
        ids
    }

    /// Run ONNX inference on phoneme IDs to produce raw audio samples.
    fn infer(&mut self, phoneme_ids: &[i64]) -> Result<Vec<f32>> {
        use ndarray::{Array1, Array2};

        let seq_len = phoneme_ids.len();
        let input = Array2::from_shape_vec((1, seq_len), phoneme_ids.to_vec())
            .context("Failed to create input tensor")?;
        let input_lengths = Array1::from_vec(vec![seq_len as i64]);
        let scales = Array1::from_vec(vec![
            self.config.inference.noise_scale,
            self.config.inference.length_scale,
            self.config.inference.noise_w,
        ]);

        let input_value = Value::from_array(input)?;
        let lengths_value = Value::from_array(input_lengths)?;
        let scales_value = Value::from_array(scales)?;

        let outputs = if self.config.num_speakers > 1 {
            let sid = Array1::from_vec(vec![0i64]);
            let sid_value = Value::from_array(sid)?;
            self.session.run(ort::inputs![
                "input" => input_value, "input_lengths" => lengths_value,
                "scales" => scales_value, "sid" => sid_value])
                .context("ONNX inference failed (multi-speaker)")?
        } else {
            self.session.run(ort::inputs![
                "input" => input_value, "input_lengths" => lengths_value,
                "scales" => scales_value])
                .context("ONNX inference failed")?
        };

        let (_shape, raw_data) = outputs[0]
            .try_extract_tensor::<f32>()
            .context("Failed to extract output audio tensor")?;

        let samples: Vec<f32> = raw_data.iter().copied().collect();
        Ok(samples)
    }
}

/// Find the espeak-ng binary, checking common paths.
fn find_espeak_ng() -> Result<String> {
    let candidates = ["espeak-ng", "/opt/homebrew/bin/espeak-ng",
        "/usr/local/bin/espeak-ng", "/usr/bin/espeak-ng"];

    for candidate in &candidates {
        if Command::new(candidate).arg("--version").output()
            .map(|o| o.status.success()).unwrap_or(false) {
            return Ok(candidate.to_string());
        }
    }
    anyhow::bail!("espeak-ng not found. Install it.")
}

/// Resample mono f32 audio using a high-quality sinc interpolator.
fn resample(input: &[f32], from_rate: u32, to_rate: u32) -> Result<Vec<f32>> {
    use rubato::{Resampler, SincFixedIn, SincInterpolationParameters,
        SincInterpolationType, WindowFunction};

    if input.is_empty() { return Ok(Vec::new()); }

    let params = SincInterpolationParameters {
        sinc_len: 256, f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };
    let ratio = to_rate as f64 / from_rate as f64;

    let mut resampler = SincFixedIn::<f32>::new(
        ratio, 2.0, params, input.len(), 1)
        .context("Failed to create resampler")?;

    let waves_in = vec![input.to_vec()];
    let waves_out = resampler.process(&waves_in, None)
        .context("Resampling failed")?;

    Ok(waves_out.into_iter().next().unwrap_or_default())
}