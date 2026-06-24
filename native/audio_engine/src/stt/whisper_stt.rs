use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use crate::tracelog;

/// Persistent Whisper STT process. Model stays loaded in Python memory.
/// No new process per request = no window flashing.
pub struct WhisperSttSession {
    child: Mutex<Child>,
    buffer: Vec<f32>,
    sample_rate: u32,
    model_name: String,
}

const BUFFER_SECS: f32 = 5.0;

impl WhisperSttSession {
    pub fn new(sample_rate: u32, model_name: &str) -> Result<Self> {
        let mut child = Command::new("python")
            .arg("whisper_stt.py")
            .env("WHISPER_MODEL", model_name)
            .env("WHISPER_DEVICE", "cpu")
            .env("WHISPER_COMPUTE", "int8")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to start whisper_stt.py")?;

        // Wait for model to load
        let mut stderr = child.stderr.take().unwrap();
        let mut buf = [0u8; 1024];
        let mut model_loaded = false;
        let start = std::time::Instant::now();
        while start.elapsed().as_secs() < 30 {
            let n = stderr.read(&mut buf).unwrap_or(0);
            if n > 0 {
                let msg = String::from_utf8_lossy(&buf[..n]);
                if msg.contains("Model loaded") {
                    model_loaded = true;
                    tracelog::trace("stt", "STT", &format!("Whisper model loaded: {}", msg.trim()));
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if !model_loaded {
            tracelog::trace("stt", "STT", "Whisper model may still be loading...");
        }

        Ok(Self {
            child: Mutex::new(child),
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

        let mut child = self.child.lock().unwrap();

        // Send header (sample_rate as u32 LE) + audio data
        if let Some(ref mut stdin) = child.stdin {
            let _ = stdin.write_all(&self.sample_rate.to_le_bytes());
            for &sample in &audio {
                let _ = stdin.write_all(&sample.to_le_bytes());
            }
            let _ = stdin.flush();
        }

        // Read response
        let mut stdout = child.stdout.take().unwrap();
        let mut response = String::new();
        let read_start = std::time::Instant::now();
        let mut buf = [0u8; 4096];
        while read_start.elapsed().as_secs() < 60 {
            match stdout.read(&mut buf) {
                Ok(0) => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    continue;
                }
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]);
                    response.push_str(&chunk);
                    if response.contains('\n') {
                        break;
                    }
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    continue;
                }
            }
        }

        // Put stdout back
        child.stdout = Some(stdout);

        let response = response.trim().to_string();
        let latency_ms = start.elapsed().as_millis() as u64;

        let result: WhisperResponse = match serde_json::from_str(&response) {
            Ok(r) => r,
            Err(e) => {
                tracelog::trace("stt", "ERROR", &format!("Whisper parse error: {} | raw: {}", e, response));
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

impl Drop for WhisperSttSession {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
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
