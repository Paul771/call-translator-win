/// Persistent local Whisper STT session.
///
/// Spawns a long-lived `whisper_worker.py` subprocess that loads the model
/// once. Audio chunks are sent as base64-encoded PCM via stdin/stdout with
/// 4-byte big-endian length-prefix protocol (matching the Rust↔Elixir wire).
use std::io::{BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use anyhow::{Context, Result};

use crate::tracelog;

pub struct WhisperSttSession {
    worker: Option<Child>,
    stdin: Option<Box<dyn Write + Send>>,
    reader: Option<BufReader<Box<dyn Read + Send>>>,
    buffer: Vec<f32>,
    sample_rate: u32,
    model_name: String,
}

const BUFFER_SECS: f32 = 2.0;

impl WhisperSttSession {
    pub fn new(sample_rate: u32, model_name: &str) -> Result<Self> {
        let script_path = std::env::current_dir()
            .unwrap_or_default()
            .join("whisper_worker.py");

        let mut cmd = Command::new(r"C:\Program Files\Python312\pythonw.exe");
        cmd.arg(script_path.to_string_lossy().replace('\\', "\\\\"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut worker = cmd.spawn()
            .context("Failed to spawn whisper_worker.py")?;

        let stdin = Box::new(worker.stdin.take()
            .context("Worker stdin not available")?);
        let stdout = worker.stdout.take()
            .context("Worker stdout not available")?;
        let mut reader = BufReader::new(Box::new(stdout) as Box<dyn Read + Send>);

        // Send init command
        let init_msg = serde_json::json!({
            "cmd": "init",
            "model": model_name
        });
        write_msg(&mut *stdin, &init_msg)?;

        // Read init response
        let resp: serde_json::Value = read_msg(&mut reader)
            .context("Worker init: no response")?;
        if resp.get("status").and_then(|v| v.as_str()) != Some("ok") {
            anyhow::bail!("Worker init failed: {:?}", resp);
        }

        tracelog::trace("stt", "STT", &format!(
            "Persistent Whisper worker started (model={}, rate={}Hz)", model_name, sample_rate
        ));

        Ok(Self {
            worker: Some(worker),
            stdin: Some(stdin),
            reader: Some(reader),
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

        // Skip very quiet audio to prevent hallucinations
        let rms = (audio.iter().map(|s| s * s).sum::<f32>() / audio.len().max(1) as f32).sqrt();
        if rms < 0.25 {
            return Ok(None);
        }

        let start = std::time::Instant::now();

        // Convert f32 to i16 PCM
        let mut pcm = Vec::with_capacity(audio.len() * 2);
        for &s in &audio {
            let i = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            pcm.extend_from_slice(&i.to_le_bytes());
        }

        let stdin = self.stdin.as_mut()
            .context("Worker stdin closed")?;
        let reader = self.reader.as_mut()
            .context("Worker stdout closed")?;

        let msg = serde_json::json!({
            "cmd": "transcribe",
            "audio_b64": base64_enc(&pcm)
        });
        write_msg(stdin, &msg)?;

        // Read response with a timeout via a channel.
        // Spawn a short-lived thread so the pipeline loop is never blocked on pipe IO.
        let (tx, rx) = std::sync::mpsc::channel();
        let mut reader_taken = std::mem::take(reader);
        std::thread::spawn(move || {
            let result = read_msg(&mut reader_taken).map_err(|e| anyhow::anyhow!("{}", e));
            let _ = tx.send((result, reader_taken));
        });

        let timeout = std::time::Duration::from_secs(8);
        match rx.recv_timeout(timeout) {
            Ok((Ok(resp), rdr)) => {
                self.reader = Some(rdr);
                let latency_ms = start.elapsed().as_millis() as u64;

                let text = resp.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let lang = resp.get("language").and_then(|v| v.as_str()).unwrap_or("??").to_string();

                if !text.trim().is_empty() {
                    tracelog::trace("stt", "STT", &format!(
                        "WHISPER stt={}ms lang={} text='{}'",
                        latency_ms, lang, text
                    ));
                    Ok(Some(super::SttResult {
                        text,
                        stt_latency_ms: latency_ms,
                        kind: super::SttResultKind::Final,
                    }))
                } else {
                    Ok(None)
                }
            }
            Ok((Err(e), rdr)) => {
                self.reader = Some(rdr);
                Err(anyhow::anyhow!("Whisper worker response error: {:#}", e))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                tracelog::trace("stt", "STT", "Whisper worker timeout — killing and re-creating on next poll");
                // The reader thread is still running and owns the reader/stdin handles.
                // Kill the worker process; the thread will fail and drop the handles.
                self.reader = None;
                self.stdin = None;
                if let Some(mut w) = self.worker.take() {
                    let _ = w.kill();
                    let _ = w.wait();
                }
                Ok(None)
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err(anyhow::anyhow!("Whisper worker response thread disconnected"))
            }
        }
    }

    pub fn close(&mut self) {
        self.stdin = None;
        self.reader = None;
        if let Some(mut w) = self.worker.take() {
            let _ = w.kill();
            let _ = w.wait();
        }
    }
}

// --- Wire helpers ---

fn write_msg(w: &mut dyn Write, msg: &serde_json::Value) -> Result<()> {
    let payload = serde_json::to_vec(msg)?;
    let len = payload.len() as u32;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(&payload)?;
    w.flush()?;
    Ok(())
}

fn read_msg(r: &mut BufReader<Box<dyn Read + Send>>) -> Result<serde_json::Value> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).context("Failed to read message length")?;
    let len = u32::from_be_bytes(len_buf) as usize;

    // BufReader::fill_buf / consume approach to avoid allocating for reads
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload).context("Failed to read message payload")?;

    serde_json::from_slice(&payload).context("Failed to parse worker response")
}

fn base64_enc(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(data)
}
