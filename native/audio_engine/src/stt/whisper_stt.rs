use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

use crate::tracelog;

pub struct WhisperSttSession {
    buffer: Vec<f32>,
    sample_rate: u32,
    model_name: String,
}

const BUFFER_SECS: f32 = 2.0;

impl WhisperSttSession {
    pub fn new(sample_rate: u32, model_name: &str) -> Result<Self> {
        tracelog::trace("stt", "STT", &format!(
            "Local Whisper STT initialized (model={}, rate={}Hz)", model_name, sample_rate
        ));
        Ok(Self {
            buffer: Vec::new(),
            sample_rate,
            model_name: model_name.to_string(),
        })
    }

    pub fn send_audio(&mut self, samples: &[f32]) -> Result<()> {
        self.buffer.extend_from_slice(samples);
        Ok(())
    }

    pub fn poll_transcript(&mut self) -> Result<Option<super::SttResult>> {
        let buffer_duration = self.buffer.len() as f32 / self.sample_rate as f32;
        if buffer_duration < BUFFER_SECS {
            return Ok(None);
        }

        let audio = std::mem::take(&mut self.buffer);
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

        // Run Whisper with WAV file path
        let wav_str = wav_path.to_string_lossy().replace('\\', "\\\\");
        let model = &self.model_name;
        let duration = audio.len() as f32 / self.sample_rate as f32;
        let python_code = format!(
            "import json; from faster_whisper import WhisperModel; \
             m = WhisperModel(r'{model}', device='cpu', compute_type='int8'); \
             segs, info = m.transcribe(r'{wav_str}', beam_size=1, language=None, vad_filter=True, \
             vad_parameters=dict(min_silence_duration_ms=300, speech_pad_ms=300), \
             no_speech_threshold=0.3, log_prob_threshold=-2.0, condition_on_previous_text=False); \
             t = ' '.join(s.text.strip() for s in segs if len(s.text.strip()) >= 2); \
             print(json.dumps({{'text': t, 'language': info.language, 'duration': {duration} }}))"
        );

        let output = Command::new("python")
            .args(["-c", &python_code])
            .env("WHISPER_MODEL", &self.model_name)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .context("Failed to run Whisper")?;

        let latency_ms = start.elapsed().as_millis() as u64;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        // Clean up WAV file
        let _ = std::fs::remove_file(&wav_path);

        if !stderr.trim().is_empty() {
            tracelog::trace("stt", "STT", &format!("Whisper stderr: {}", stderr.trim().lines().next().unwrap_or("")));
        }

        if stdout.trim().is_empty() {
            tracelog::trace("stt", "STT", &format!("Whisper empty output after {}ms", latency_ms));
            return Ok(None);
        }

        let result: WhisperResponse = match serde_json::from_str(&stdout) {
            Ok(r) => r,
            Err(e) => {
                tracelog::trace("stt", "ERROR", &format!("Whisper parse error: {} | stdout: {}", e, &stdout[..200.min(stdout.len())]));
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
