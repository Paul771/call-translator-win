use std::io::ErrorKind;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use log::info;
use serde::Deserialize;
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};
use crate::tracelog;

fn log_file(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true).append(true).open("deepgram_debug.log")
    {
        use std::io::Write;
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().as_secs();
        let _ = writeln!(f, "[{}] [stt] {}", secs, msg);
    }
}

pub struct DeepgramStt {
    api_key: String,
    language: String,
    endpointing_ms: u32,
}

impl DeepgramStt {
    pub fn new(api_key: String, language: String, endpointing_ms: u32) -> Self {
        let language = match language.as_str() {
            "pt" => "pt-BR",
            "no" => "nb",
            code => code,
        }.to_string();
        Self { api_key, language, endpointing_ms }
    }

    pub fn create_session(&self, sample_rate: u32) -> Result<DeepgramSession> {
        let url_str = if self.language == "any" {
            format!(
                "wss://api.deepgram.com/v1/listen\
                 ?model=nova-2-general\
                 &encoding=linear16\
                 &sample_rate={}\
                 &channels=1\
                 &multichannel=false\
                 &interim_results=true\
                 &endpointing={}\
                 &utterance_end_ms=1500\
                 &vad_events=true",
                sample_rate, self.endpointing_ms
            )
        } else {
            format!(
                "wss://api.deepgram.com/v1/listen\
                 ?model=nova-2-general\
                 &language={}\
                 &encoding=linear16\
                 &sample_rate={}\
                 &channels=1\
                 &multichannel=false\
                 &interim_results=true\
                 &endpointing={}\
                 &utterance_end_ms=1500\
                 &vad_events=true",
                self.language, sample_rate, self.endpointing_ms
            )
        };

        tracelog::trace("stt", "STT", &format!("connecting lang={} rate={}Hz endpointing={}ms",
            self.language, sample_rate, self.endpointing_ms));

        let mut request = url_str
            .into_client_request()
            .context("Failed to build Deepgram request")?;
        request.headers_mut().insert(
            "Authorization",
            format!("Token {}", self.api_key)
                .parse()
                .context("Invalid API key header value")?,
        );

        info!(
            "Connecting to Deepgram (lang={}, {}Hz, endpointing={}ms)...",
            self.language, sample_rate, self.endpointing_ms
        );
        log_file(&format!("connecting lang={} rate={} endpointing={}ms", self.language, sample_rate, self.endpointing_ms));

        let (connect_tx, connect_rx) = std::sync::mpsc::channel();
        let connect_timeout = Duration::from_secs(15);
        std::thread::spawn(move || {
            let result = connect(request);
            let _ = connect_tx.send(result);
        });
        let (mut ws, resp) = match connect_rx.recv_timeout(connect_timeout) {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                log_file(&format!("connect FAILED: {}", e));
                bail!("Failed to connect to Deepgram WebSocket: {}", e);
            }
            Err(_) => {
                log_file(&format!("connect TIMEOUT after {}s", connect_timeout.as_secs()));
                bail!("Deepgram WebSocket connection timed out after {}s", connect_timeout.as_secs());
            }
        };
        log_file(&format!("WS connected status={}", resp.status()));
        tracelog::trace("stt", "STT", &format!("WS connected status={}", resp.status()));

        // Non-blocking mode for real-time audio streaming.
        // Audio is sent every loop iteration to keep Deepgram connection alive.
        set_nonblocking(&mut ws)?;

        info!("Deepgram session connected (non-blocking mode)");
        Ok(DeepgramSession {
            ws,
            last_send_time: Instant::now(),
            sample_rate,
            pending_audio: Vec::new(),
        })
    }
}

const MAX_PENDING_AUDIO: usize = 65536;

pub struct DeepgramSession {
    ws: WebSocket<MaybeTlsStream<std::net::TcpStream>>,
    last_send_time: Instant,
    #[allow(dead_code)]
    sample_rate: u32,
    pending_audio: Vec<u8>,
}

pub struct SttResult {
    pub text: String,
    pub stt_latency_ms: u64,
}

impl DeepgramSession {
    pub fn send_audio(&mut self, samples: &[f32]) -> Result<()> {
        let bytes: Vec<u8> = samples
            .iter()
            .flat_map(|&s| {
                let i = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                i.to_le_bytes()
            })
            .collect();

        match self.ws.send(Message::Binary(bytes)) {
            Ok(()) => {
                self.last_send_time = Instant::now();
                Ok(())
            }
            Err(tungstenite::Error::Io(ref e)) if e.kind() == ErrorKind::WouldBlock => {
                // Buffer for next flush
                let rebuf: Vec<u8> = samples
                    .iter()
                    .flat_map(|&s| {
                        let i = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                        i.to_le_bytes()
                    })
                    .collect();
                self.pending_audio.extend_from_slice(&rebuf);
                if self.pending_audio.len() > MAX_PENDING_AUDIO {
                    let excess = self.pending_audio.len() - MAX_PENDING_AUDIO;
                    self.pending_audio.drain(..excess);
                }
                Ok(())
            }
            Err(e) => {
                tracelog::trace("stt", "ERROR", &format!("WS send failed: {}", e));
                Err(anyhow::anyhow!("Failed to send audio to Deepgram: {}", e))
            }
        }
    }

    pub fn flush_pending(&mut self) -> Result<()> {
        if self.pending_audio.is_empty() {
            return Ok(());
        }
        match self.ws.send(Message::Binary(self.pending_audio.clone())) {
            Ok(()) => {
                self.pending_audio.clear();
                self.last_send_time = Instant::now();
                Ok(())
            }
            Err(tungstenite::Error::Io(ref e)) if e.kind() == ErrorKind::WouldBlock => {
                Ok(())
            }
            Err(e) => {
                Err(anyhow::anyhow!("Failed to flush pending audio: {}", e))
            }
        }
    }

    pub fn poll_transcript(&mut self) -> Result<Option<SttResult>> {
        loop {
            match self.ws.read() {
                Ok(Message::Text(text)) => {
                    match serde_json::from_str::<DgResponse>(&text) {
                        Ok(resp) => {
                            let transcript = resp
                                .channel
                                .as_ref()
                                .and_then(|c| c.get("alternatives"))
                                .and_then(|alt| alt.as_array())
                                .and_then(|arr| arr.first())
                                .and_then(|a| a.get("transcript"))
                                .and_then(|t| t.as_str())
                                .unwrap_or_default()
                                .to_string();
                            let is_final = resp.is_final == Some(true);
                            if is_final && !transcript.trim().is_empty() {
                                let since_last = self.last_send_time.elapsed().as_millis() as u64;
                                tracelog::trace("stt", "STT", &format!("FINAL stt={}ms text='{}'", since_last, transcript));
                                return Ok(Some(SttResult { text: transcript, stt_latency_ms: since_last }));
                            } else if is_final {
                                return Ok(None);
                            } else {
                                if !transcript.trim().is_empty() {
                                    tracelog::trace("stt", "STT", &format!("interim: '{}'", transcript));
                                }
                            }
                        }
                        Err(e) => {
                            let preview: String = text.chars().take(200).collect();
                            tracelog::trace("stt", "ERROR", &format!("JSON parse error: {} | raw: {}", e, preview));
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    let _ = self.ws.send(Message::Pong(data));
                }
                Ok(Message::Close(_)) => {
                    tracelog::trace("stt", "ERROR", "Deepgram WS closed by server");
                    bail!("Deepgram connection closed by server");
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if e.kind() == ErrorKind::WouldBlock =>
                {
                    return Ok(None);
                }
                Err(e) => {
                    tracelog::trace("stt", "ERROR", &format!("WS read error: {}", e));
                    bail!("Deepgram WebSocket error: {}", e);
                }
            }
        }
    }

    pub fn close(&mut self) {
        self.pending_audio.clear();
        let _ = self.ws.send(Message::Binary(vec![]));
        let _ = self.ws.close(None);
    }
}

#[derive(Deserialize)]
struct DgResponse {
    #[serde(default)]
    is_final: Option<bool>,
    #[serde(default)]
    channel: Option<serde_json::Value>,
}

fn set_nonblocking(ws: &mut WebSocket<MaybeTlsStream<std::net::TcpStream>>) -> Result<()> {
    match ws.get_mut() {
        MaybeTlsStream::Plain(s) => {
            s.set_nonblocking(true)
                .context("set_nonblocking (plain)")?;
        }
        MaybeTlsStream::NativeTls(s) => {
            s.get_ref()
                .set_nonblocking(true)
                .context("set_nonblocking (tls)")?;
        }
        _ => {}
    }
    Ok(())
}
