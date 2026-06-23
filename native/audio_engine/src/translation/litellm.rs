use anyhow::{bail, Context, Result};
use serde::Deserialize;

pub struct LiteLlmTranslator {
    base_url: String,
    api_key: String,
    model: String,
    client: reqwest::blocking::Client,
}

impl LiteLlmTranslator {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .context("Failed to build HTTP client for LiteLLM")?;

        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            client,
        })
    }

    pub fn translate(&self, text: &str, from: &str, to: &str) -> Result<String> {
        let system_prompt = format!(
            "You are a machine translation API. \
             Your ONLY output is the {to} translation of the user's {from} text. \
             No explanations. No greetings. No disclaimers. \
             Just the translation, word for word.",
            from = from,
            to = to,
        );

        let body = serde_json::json!({
            "model": &self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": text}
            ],
            "max_tokens": 128,
            "temperature": 0.1
        });

        let url = format!("{}/chat/completions", self.base_url);
        let body_string = serde_json::to_string(&body).unwrap_or_default();

        let debug = format!(
            "[LITELLM] POST {} model='{}' text='{}'",
            url, &self.model, text
        );
        eprintln!("{}", debug);
        crate::translation::log_to_file("litellm_debug.log", &debug);

        let mut last_error = None;
        for attempt in 1..=3u32 {
            if attempt > 1 {
                let delay_ms = 500u64 * 2u64.pow(attempt - 2);
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }

            let response = self.client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json; charset=utf-8")
                .body(body_string.clone())
                .send();

            match response {
                Ok(r) if r.status().is_success() => {
                    let bytes = r.bytes().unwrap_or_default();
                    let response_body = String::from_utf8(bytes.to_vec()).unwrap_or_default();

                    let resp = match serde_json::from_str::<LiteLlmResponse>(&response_body) {
                        Ok(resp) => resp,
                        Err(e) => {
                            let msg = format!("[LITELLM] Parse error: {}", e);
                            eprintln!("{}", msg);
                            crate::translation::log_to_file("litellm_debug.log", &msg);
                            last_error = Some(anyhow::anyhow!("{}", msg));
                            continue;
                        }
                    };

                    let translated = resp.choices.into_iter().next()
                        .map(|c| c.message.content.trim().to_string())
                        .unwrap_or_default()
                        .replace("<think>", "")
                        .replace("</think>", "");

                    let translated = crate::translation::clean_groq_output(&translated);

                    let out = format!("[LITELLM] OK: '{}'", translated);
                    eprintln!("{}", out);
                    crate::translation::log_to_file("litellm_debug.log", &out);

                    return Ok(translated);
                }
                Ok(r) => {
                    let status = r.status();
                    let bytes = r.bytes().unwrap_or_default();
                    let body_text = String::from_utf8(bytes.to_vec()).unwrap_or_default();
                    let msg = format!("[LITELLM] Error {}: {}", status, body_text);
                    eprintln!("{}", msg);
                    crate::translation::log_to_file("litellm_debug.log", &msg);
                    if status.as_u16() >= 400 && status.as_u16() < 500 {
                        bail!("LiteLLM error {}: {}", status, body_text);
                    }
                    last_error = Some(anyhow::anyhow!("{}", msg));
                }
                Err(e) => {
                    let msg = format!("[LITELLM] Request failed: {:#}", e);
                    eprintln!("{}", msg);
                    crate::translation::log_to_file("litellm_debug.log", &msg);
                    last_error = Some(anyhow::anyhow!("{}", msg));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("LiteLLM all retries exhausted")))
    }
}

#[derive(Deserialize)]
struct LiteLlmResponse {
    choices: Vec<LiteLlmChoice>,
}

#[derive(Deserialize)]
struct LiteLlmChoice {
    message: LiteLlmMessage,
}

#[derive(Deserialize)]
struct LiteLlmMessage {
    content: String,
}
