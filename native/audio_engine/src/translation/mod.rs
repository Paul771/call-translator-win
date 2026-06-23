pub mod yandex;
pub mod litellm;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationDirection {
    pub from_code: String,
    pub from_name: String,
    pub to_code: String,
    pub to_name: String,
}

impl TranslationDirection {
    pub fn new(from: &str, to: &str) -> Self {
        Self {
            from_code: from.to_string(),
            from_name: lang_name(from).to_string(),
            to_code: to.to_string(),
            to_name: lang_name(to).to_string(),
        }
    }
}

impl std::fmt::Display for TranslationDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}→{}", self.from_code, self.to_code)
    }
}

fn lang_name(code: &str) -> &str {
    match code {
        "ar" => "Arabic", "ca" => "Catalan", "cs" => "Czech",
        "cy" => "Welsh", "da" => "Danish", "de" => "German",
        "el" => "Greek", "en" => "English", "es" => "Spanish",
        "fa" => "Persian", "fi" => "Finnish", "fr" => "French",
        "hi" => "Hindi", "hu" => "Hungarian", "is" => "Icelandic",
        "it" => "Italian", "ja" => "Japanese", "ka" => "Georgian",
        "kk" => "Kazakh", "ko" => "Korean", "lb" => "Luxembourgish",
        "lv" => "Latvian", "ml" => "Malayalam", "ne" => "Nepali",
        "nl" => "Dutch", "no" => "Norwegian", "pl" => "Polish",
        "pt" => "Portuguese", "ro" => "Romanian", "ru" => "Russian",
        "sk" => "Slovak", "sl" => "Slovenian", "sr" => "Serbian",
        "sv" => "Swedish", "sw" => "Swahili", "tr" => "Turkish",
        "uk" => "Ukrainian", "vi" => "Vietnamese", "zh" => "Chinese",
        _ => code,
    }
}

#[derive(Clone)]
pub struct TranslationEngine {
    groq_key: String,
    yandex_key: String,
    yandex_folder_id: String,
    litellm_base_url: String,
    litellm_api_key: String,
    litellm_model: String,
    provider: String,
    client: reqwest::blocking::Client,
}

impl TranslationEngine {
    pub fn new(groq_key: &str, yandex_key: &str, yandex_folder_id: &str, provider: &str,
               litellm_base_url: &str, litellm_api_key: &str, litellm_model: &str) -> Result<Self> {
        eprintln!("[TRANSLATION] Provider: '{}'", provider);
        eprintln!("[TRANSLATION] Using GROQ_API_KEY: {}... (len={})",
            if groq_key.len() > 4 { &groq_key[..4] } else { "?" },
            groq_key.len());
        eprintln!("[TRANSLATION] Using YANDEX_API_KEY: {}... (len={})",
            if yandex_key.len() > 4 { &yandex_key[..4] } else { "?" },
            yandex_key.len());
        eprintln!("[TRANSLATION] LiteLLM: url='{}' model='{}'",
            litellm_base_url, litellm_model);
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .connect_timeout(std::time::Duration::from_secs(15))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to build HTTP client")?;
        Ok(Self {
            groq_key: groq_key.to_string(),
            yandex_key: yandex_key.to_string(),
            yandex_folder_id: yandex_folder_id.to_string(),
            litellm_base_url: litellm_base_url.to_string(),
            litellm_api_key: litellm_api_key.to_string(),
            litellm_model: litellm_model.to_string(),
            provider: provider.to_string(),
            client,
        })
    }

    pub fn translate(&self, text: &str, direction: &TranslationDirection) -> Result<String> {
        if text.trim().is_empty() {
            return Ok(String::new());
        }

        match self.provider.as_str() {
            "groq" => {
                if self.groq_key.len() > 4 {
                    return self.translate_groq(text, direction);
                }
                bail!("Groq selected but no valid API key")
            }
            "yandex" => {
                if self.yandex_key.len() > 4 {
                    return self.translate_yandex(text, direction);
                }
                bail!("Yandex selected but no valid API key")
            }
            "litellm" => {
                if self.litellm_api_key.len() > 2 {
                    return self.translate_litellm(text, direction);
                }
                bail!("LiteLLM selected but no valid API key")
            }
            _ => {} // "auto" — try Groq → LiteLLM → Yandex
        }

        // Auto mode: try Groq first
        if self.groq_key.len() > 4 {
            match self.translate_groq(text, direction) {
                Ok(result) => return Ok(result),
                Err(e) => {
                    let err_line = format!("[FALLBACK] Groq failed: {:#}, trying LiteLLM", e);
                    eprintln!("{}", err_line);
                    log_to_file("groq_debug.log", &err_line);
                }
            }
        }

        // Fallback to LiteLLM
        if self.litellm_api_key.len() > 2 && !self.litellm_base_url.is_empty() {
            match self.translate_litellm(text, direction) {
                Ok(result) => return Ok(result),
                Err(e) => {
                    let err_line = format!("[FALLBACK] LiteLLM failed: {:#}, trying Yandex", e);
                    eprintln!("{}", err_line);
                    log_to_file("litellm_debug.log", &err_line);
                }
            }
        }

        // Fallback to Yandex
        if self.yandex_key.len() > 4 {
            return self.translate_yandex(text, direction);
        }

        bail!("No valid API keys available for translation")
    }

    fn translate_litellm(&self, text: &str, direction: &TranslationDirection) -> Result<String> {
        let translator = litellm::LiteLlmTranslator::new(
            &self.litellm_base_url,
            &self.litellm_api_key,
            &self.litellm_model,
        )?;
        translator.translate(text, &direction.from_code, &direction.to_code)
    }

    fn translate_groq(&self, text: &str, direction: &TranslationDirection) -> Result<String> {
        let system_prompt = format!(
            "You are a machine translation API. \
             Your ONLY output is the {to} translation of the user's {from} text. \
             No explanations. No greetings. No disclaimers. \
             Just the translation, word for word.\n\
             Examples:\n\
             User: привет как дела\n\
             Assistant: hello how are you\n\
             User: мне нужна помощь\n\
             Assistant: i need help",
            from = direction.from_name,
            to = direction.to_name,
        );

        let model = "llama-3.1-8b-instant";

        let body = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user",   "content": text}
            ],
            "max_tokens": 40,
            "temperature": 0.1
        });

        let body_string = serde_json::to_string(&body).unwrap_or_default();

        let debug_line = format!(
            "🔑 KEY={}...{} (len={})\n📡 GROQ req: model={}, text={}",
            &self.groq_key[..4.min(self.groq_key.len())],
            &self.groq_key[32..36.min(self.groq_key.len())],
            self.groq_key.len(),
            model,
            text,
        );
        eprintln!("{}", debug_line);
        log_to_file("groq_debug.log", &debug_line);

        // Retry loop: 3 attempts with exponential backoff (1s, 2s, 4s)
        let max_attempts = 3;
        let mut last_error = None;
        for attempt in 1..=max_attempts {
            if attempt > 1 {
                let delay_ms = 1000u64 * 2u64.pow(attempt as u32 - 2);
                let retry_msg = format!("🔁 GROQ retry #{}/{} after {}ms", attempt, max_attempts, delay_ms);
                eprintln!("{}", retry_msg);
                log_to_file("groq_debug.log", &retry_msg);
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }

            let response = self.client
                .post("https://api.groq.com/openai/v1/chat/completions")
                .header("Authorization", format!("Bearer {}", self.groq_key))
                .header("Content-Type", "application/json; charset=utf-8")
                .body(body_string.clone())
                .send();

            match response {
                Ok(r) if r.status().is_success() => {
                    let bytes = r.bytes().unwrap_or_default();
                    let response_body = String::from_utf8(bytes.to_vec()).unwrap_or_default();

                    let groq_resp = match serde_json::from_str::<GroqResponse>(&response_body) {
                        Ok(resp) => resp,
                        Err(e) => {
                            let err_line = format!("❌ GROQ parse error: {}", e);
                            eprintln!("{}", err_line);
                            log_to_file("groq_debug.log", &err_line);
                            last_error = Some(anyhow::anyhow!("{}", err_line));
                            continue;
                        }
                    };

                    let translated = groq_resp.choices.into_iter().next()
                        .map(|c| c.message.content.trim().to_string())
                        .unwrap_or_default()
                        .replace("<think>", "")
                        .replace("</think>", "");

                    let translated = clean_groq_output(&translated);

                    let out_line = format!("✅ GROQ Output: {}", translated);
                    eprintln!("{}", out_line);
                    log_to_file("groq_debug.log", &out_line);

                    return Ok(translated);
                }
                Ok(r) => {
                    let status = r.status();
                    let bytes = r.bytes().unwrap_or_default();
                    let body_text = String::from_utf8(bytes.to_vec()).unwrap_or_default();
                    let err_line = format!("❌ GROQ error {}: {}", status, body_text);
                    eprintln!("{}", err_line);
                    log_to_file("groq_debug.log", &err_line);
                    if status.as_u16() >= 400 && status.as_u16() < 500 {
                        bail!("Groq error {}: {}", status, body_text);
                    }
                    last_error = Some(anyhow::anyhow!("Groq error {}: {}", status, body_text));
                }
                Err(e) => {
                    let err_line = format!("❌ GROQ request failed: {:#}", e);
                    eprintln!("{}", err_line);
                    log_to_file("groq_debug.log", &err_line);
                    last_error = Some(anyhow::anyhow!("{}", err_line));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("GROQ all retries exhausted")))
    }

    fn translate_yandex(&self, text: &str, direction: &TranslationDirection) -> Result<String> {
        let translator = yandex::YandexTranslator::new(&self.yandex_key, &self.yandex_folder_id)?;

        let result = translator.translate(text, &direction.from_code, &direction.to_code)?;

        let out_line = format!("✅ YANDEX Output: {}", result);
        eprintln!("{}", out_line);
        log_to_file("groq_debug.log", &out_line);

        Ok(result)
    }
}

fn clean_groq_output(text: &str) -> String {
    let t = text.trim();
    // Pattern: "something" translates to "translation"
    if let Some(pos) = t.to_lowercase().find("translates to") {
        let after = &t[pos + "translates to".len()..];
        // Extract text between quotes
        if let Some(start) = after.find('"') {
            let rest = &after[start + 1..];
            if let Some(end) = rest.find('"') {
                return rest[..end].to_string();
            }
        }
        // Or first sentence after "translates to"
        let first_sentence = after.split(|c| c == '.' || c == '!' || c == '?')
            .next().unwrap_or("").trim().trim_matches('"');
        if !first_sentence.is_empty() {
            return first_sentence.to_string();
        }
    }
    // Pattern: starts with "Russian phrase" (quoted), extract the last quoted English
    let parts: Vec<&str> = t.split('"').collect();
    if parts.len() >= 3 {
        // Try last quoted segment
        if let Some(last) = parts.last() {
            let candidate = last.trim();
            if !candidate.is_empty() && candidate.len() < 100 {
                return candidate.to_string();
            }
        }
    }
    // If output contains verbose patterns, strip first sentence
    let verbose_patterns = ["okay", "let's see", "the user said", "the user wrote",
                            "translates to", "breaking it down", "first,", "i need to",
                            "putting it together", "let me break"];
    let lower = t.to_lowercase();
    if verbose_patterns.iter().any(|p| lower.contains(p)) {
        let sentences: Vec<&str> = t.split(|c| c == '.' || c == '!' || c == '?')
            .map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        for s in sentences.iter().rev() {
            let sl = s.to_lowercase();
            if !verbose_patterns.iter().any(|p| sl.contains(p)) && !s.contains('"') {
                return s.trim().to_string();
            }
        }
        return sentences.last().unwrap_or(&"").trim().to_string();
    }
    t.to_string()
}

fn log_to_file(filename: &str, line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true).append(true).open(filename)
    {
        use std::io::Write;
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().as_secs();
        let _ = writeln!(f, "[{}] {}", secs, line);
    }
}

#[derive(Deserialize)]
struct GroqResponse {
    choices: Vec<GroqChoice>,
}

#[derive(Deserialize)]
struct GroqChoice {
    message: GroqMessage,
}

#[derive(Deserialize)]
struct GroqMessage {
    content: String,
}
