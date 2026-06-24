use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

use crate::tracelog;

pub struct WhisperSttSession {
    buffer: Vec<f32>,
    sample_rate: u32,
    model_name: String,
    last_send: std::time::Instant,
}

const BUFFER_SECS: f32 = 5.0;

impl WhisperSttSession {
    pub fn new(sample_rate: u32, model_name: &str) -> Result<Self> {
        // Pre-warm: load model in background
        let model = model_name.to_string();
        let _ = std::thread::spawn(move || {
            let _ = Command::new("python")
                .args(["-c", &format!(
                    "from faster_whisper import WhisperModel; WhisperModel('{}', device='cpu', compute_type='int8'); print('[whisper] Model preloaded: {}')",
                    model, model
                )])
                .output();
        });

        tracelog::trace("stt", "STT", &format!(
            "Local Whisper STT initialized (model={}, rate={}Hz)", model_name, sample_rate
        ));

        Ok(Self {
            buffer: Vec::new(),
            sample_rate,
            model_name: model_name.to_string(),
            last_send: std::time::Instant::now(),
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

        // Write audio to Python via stdin
        let mut child = Command::new("python")
            .arg("whisper_stt.py")
            .env("WHISPER_MODEL", &self.model_name)
            .env("WHISPER_DEVICE", "cpu")
            .env("WHISPER_COMPUTE", "int8")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to start whisper_stt.py")?;

        // Send header (sample_rate as u32 LE) + audio data
        if let Some(ref mut stdin) = child.stdin {
            let header = self.sample_rate.to_le_bytes();
            let _ = stdin.write_all(&header);

            for &sample in &audio {
                let _ = stdin.write_all(&sample.to_le_bytes());
            }
        }
        drop(child.stdin.take());

        let output = child.wait_with_output()
            .context("Failed to wait for whisper_stt.py")?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !stderr.trim().is_empty() {
            tracelog::trace("stt", "STT", &format!("Whisper stderr: {}", stderr.trim()));
        }

        let result: WhisperResponse = match serde_json::from_str(&stdout) {
            Ok(r) => r,
            Err(e) => {
                tracelog::trace("stt", "ERROR", &format!("Whisper parse error: {} | raw: {}", e, stdout));
                return Err(anyhow::anyhow!("Whisper parse error: {}", e));
            }
        };

        let latency_ms = start.elapsed().as_millis() as u64;

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
    duration: f64,
}
