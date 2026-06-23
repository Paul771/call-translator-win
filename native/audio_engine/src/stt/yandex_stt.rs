use anyhow::{Context, Result};
use serde::Deserialize;

use crate::tracelog;

pub struct YandexSttSession {
    api_key: String,
    folder_id: String,
    language: String,
    client: reqwest::blocking::Client,
    buffer: Vec<f32>,
    last_send: std::time::Instant,
    sample_rate: u32,
}

const BUFFER_SECS: f32 = 2.5;

impl YandexSttSession {
    pub fn new(api_key: &str, folder_id: &str, language: &str, sample_rate: u32) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .context("Failed to build HTTP client for Yandex STT")?;

        Ok(Self {
            api_key: api_key.to_string(),
            folder_id: folder_id.to_string(),
            language: language.to_string(),
            client,
            buffer: Vec::new(),
            last_send: std::time::Instant::now(),
            sample_rate,
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
        self.last_send = std::time::Instant::now();

        let pcm_bytes: Vec<u8> = audio
            .iter()
            .flat_map(|&s| {
                let i = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                i.to_le_bytes()
            })
            .collect();

        let lang = match self.language.as_str() {
            "ru" => "ru-RU",
            "en" => "en-US",
            other => other,
        };

        let url = format!(
            "https://stt.api.cloud.yandex.net/speech/stt/v1/recognize?folderId={}&lang={}",
            self.folder_id, lang
        );

        let start = std::time::Instant::now();

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Api-Key {}", self.api_key))
            .header("Content-Type", "application/octet-stream")
            .body(pcm_bytes)
            .send();

        match response {
            Ok(r) if r.status().is_success() => {
                let body = r.text().unwrap_or_default();
                let resp: YandexSttResponse = serde_json::from_str(&body)
                    .context("Failed to parse Yandex STT response")?;

                let latency_ms = start.elapsed().as_millis() as u64;
                let text = resp.result.unwrap_or_default();

                if !text.trim().is_empty() {
                    tracelog::trace("stt", "STT", &format!(
                        "YANDEX stt={}ms text='{}'",
                        latency_ms, text
                    ));
                    Ok(Some(super::SttResult {
                        text,
                        stt_latency_ms: latency_ms,
                    }))
                } else {
                    Ok(None)
                }
            }
            Ok(r) => {
                let status = r.status();
                let body = r.text().unwrap_or_default();
                tracelog::trace("stt", "ERROR", &format!(
                    "Yandex STT error {}: {}", status, body
                ));
                Err(anyhow::anyhow!("Yandex STT error {}: {}", status, body))
            }
            Err(e) => {
                tracelog::trace("stt", "ERROR", &format!("Yandex STT request failed: {}", e));
                Err(anyhow::anyhow!("Yandex STT request failed: {}", e))
            }
        }
    }
}

#[derive(Deserialize)]
struct YandexSttResponse {
    result: Option<String>,
}
