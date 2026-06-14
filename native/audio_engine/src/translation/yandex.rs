use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct YandexTranslator {
    api_key: String,
    folder_id: String,
    client: reqwest::blocking::Client,
}

impl YandexTranslator {
    pub fn new(api_key: &str, folder_id: &str) -> Result<Self> {
        eprintln!("[YANDEX] Using API key: {}... (len={})",
            if api_key.len() >= 4 { &api_key[..4] } else { "?" },
            api_key.len());
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .context("Failed to build Yandex HTTP client")?;
        Ok(Self {
            api_key: api_key.to_string(),
            folder_id: folder_id.to_string(),
            client,
        })
    }

    pub fn translate(&self, text: &str, source_lang: &str, target_lang: &str) -> Result<String> {
        if text.trim().is_empty() {
            return Ok(String::new());
        }

        let body = YandexRequest {
            source_language_code: source_lang.to_string(),
            target_language_code: target_lang.to_string(),
            texts: vec![text.to_string()],
            folder_id: self.folder_id.clone(),
        };

        let body_string = serde_json::to_string(&body).context("Failed to serialize Yandex request")?;

        eprintln!("[YANDEX] Translating: {} -> {}: {}",
            source_lang, target_lang,
            if text.len() > 50 { &text[..50] } else { text });

        let response = self.client
            .post("https://translate.api.cloud.yandex.net/translate/v2/translate")
            .header("Authorization", format!("Api-Key {}", self.api_key))
            .header("Content-Type", "application/json; charset=utf-8")
            .body(body_string)
            .send()
            .context("Yandex translate request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let bytes = response.bytes().unwrap_or_default();
            let text = String::from_utf8(bytes.to_vec()).unwrap_or_default();
            bail!("Yandex error {}: {}", status, text);
        }

        let resp: YandexResponse = response
            .json()
            .context("Failed to parse Yandex response")?;

        let translated = resp.translations
            .into_iter()
            .next()
            .map(|t| t.text)
            .unwrap_or_default();

        eprintln!("[YANDEX] Output: {}", translated);

        Ok(translated)
    }
}

#[derive(Serialize)]
struct YandexRequest {
    source_language_code: String,
    target_language_code: String,
    texts: Vec<String>,
    folder_id: String,
}

#[derive(Deserialize)]
struct YandexResponse {
    translations: Vec<YandexTranslation>,
}

#[derive(Deserialize)]
struct YandexTranslation {
    text: String,
}
