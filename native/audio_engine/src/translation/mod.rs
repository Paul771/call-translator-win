pub mod yandex;
pub mod litellm;
#[cfg(feature = "local-mt")]
pub mod model;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::sync::Arc;

fn new_http_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(15))
        .tcp_keepalive(std::time::Duration::from_secs(30))
        .build()
        .context("Failed to build HTTP client")
}

/// Groq sometimes answers a valid request with a bare `{"error":{"message":"Not Found"}}`.
/// It is a property of the connection (a keep-alive connection pinned to a bad node fails
/// every request, a new one usually works), not of the request. A genuinely missing model
/// says "does not exist" / `model_not_found` and must not be retried.
fn is_transient_not_found(status: u16, body: &str) -> bool {
    status == 404 && !body.contains("does not exist") && !body.contains("model_not_found")
}

/// System prompt for Groq translation. Kept in a file so tests/e2e/prompt_eval.py scores the very
/// text the engine sends. Placeholders: {from}, {to} (language names).
const GROQ_PROMPT: &str = include_str!("groq_prompt.txt");

fn groq_system_prompt(from_name: &str, to_name: &str) -> String {
    GROQ_PROMPT.trim().replace("{from}", from_name).replace("{to}", to_name)
}

/// Groq model used for translation unless GROQ_MODEL says otherwise. Groq retires models
/// without notice (llama-3.1-8b-instant and llama-3.3-70b-versatile now answer
/// `model_not_found`), which is why the model is configurable.
const DEFAULT_GROQ_MODEL: &str = "openai/gpt-oss-20b";

/// Chat-completions body for one translation. Reasoning models (gpt-oss) spend completion
/// tokens on thinking before the answer, so they get `reasoning_effort: low` and a larger
/// budget; with the old 40 tokens they return an empty reply.
fn groq_request_body(model: &str, system_prompt: &str, text: &str) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user",   "content": text}
        ],
        "max_tokens": 40,
        "temperature": 0.1
    });
    if model.starts_with("openai/gpt-oss") {
        body["reasoning_effort"] = serde_json::json!("low");
        body["max_tokens"] = serde_json::json!(400);
    }
    body
}

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
    groq_model: String,
    yandex_key: String,
    yandex_folder_id: String,
    litellm_base_url: String,
    litellm_api_key: String,
    litellm_model: String,
    provider: String,
    client: reqwest::blocking::Client,
    /// Used for Groq only; replaced by a fresh connection when Groq answers a transient 404.
    groq_client: Arc<std::sync::Mutex<reqwest::blocking::Client>>,
    yandex: Arc<std::sync::OnceLock<yandex::YandexTranslator>>,
    litellm: Arc<std::sync::OnceLock<litellm::LiteLlmTranslator>>,
    #[cfg(feature = "local-mt")]
    mt_ruen_path: String,
    #[cfg(feature = "local-mt")]
    mt_enru_path: String,
    #[cfg(feature = "local-mt")]
    mt_ruen: Arc<std::sync::OnceLock<model::TranslationModel>>,
    #[cfg(feature = "local-mt")]
    mt_enru: Arc<std::sync::OnceLock<model::TranslationModel>>,
}

impl TranslationEngine {
    pub fn new(groq_key: &str, yandex_key: &str, yandex_folder_id: &str, provider: &str,
               litellm_base_url: &str, litellm_api_key: &str, litellm_model: &str,
               #[cfg(feature = "local-mt")] mt_ruen_path: &str,
               #[cfg(feature = "local-mt")] mt_enru_path: &str) -> Result<Self> {
        let groq_model = std::env::var("GROQ_MODEL")
            .ok()
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| DEFAULT_GROQ_MODEL.to_string());
        eprintln!("[TRANSLATION] Provider: '{}', Groq model: '{}'", provider, groq_model);
        eprintln!("[TRANSLATION] Using GROQ_API_KEY: {}... (len={})",
            if groq_key.len() > 4 { &groq_key[..4] } else { "?" },
            groq_key.len());
        eprintln!("[TRANSLATION] Using YANDEX_API_KEY: {}... (len={})",
            if yandex_key.len() > 4 { &yandex_key[..4] } else { "?" },
            yandex_key.len());
        eprintln!("[TRANSLATION] LiteLLM: url='{}' model='{}'",
            litellm_base_url, litellm_model);
        #[cfg(feature = "local-mt")]
        eprintln!("[TRANSLATION] Local MT: ru->en='{}', en->ru='{}'",
            mt_ruen_path, mt_enru_path);
        let client = new_http_client()?;
        let groq_client = Arc::new(std::sync::Mutex::new(new_http_client()?));
        Ok(Self {
            groq_key: groq_key.to_string(),
            groq_model,
            yandex_key: yandex_key.to_string(),
            yandex_folder_id: yandex_folder_id.to_string(),
            litellm_base_url: litellm_base_url.to_string(),
            litellm_api_key: litellm_api_key.to_string(),
            litellm_model: litellm_model.to_string(),
            provider: provider.to_string(),
            client,
            groq_client,
            yandex: Arc::new(std::sync::OnceLock::new()),
            litellm: Arc::new(std::sync::OnceLock::new()),
            #[cfg(feature = "local-mt")]
            mt_ruen_path: mt_ruen_path.to_string(),
            #[cfg(feature = "local-mt")]
            mt_enru_path: mt_enru_path.to_string(),
            #[cfg(feature = "local-mt")]
            mt_ruen: Arc::new(std::sync::OnceLock::new()),
            #[cfg(feature = "local-mt")]
            mt_enru: Arc::new(std::sync::OnceLock::new()),
        })
    }

    pub fn translate(&self, text: &str, direction: &TranslationDirection) -> Result<(String, String)> {
        if text.trim().is_empty() {
            return Ok((String::new(), "empty".into()));
        }

        let provider_tag = |name: &str| name.to_string();

        match self.provider.as_str() {
            "groq" => {
                if self.groq_key.len() > 4 {
                    return self.translate_groq(text, direction)
                        .map(|t| (t, provider_tag("groq")));
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
                    return self.translate_litellm(text, direction)
                        .map(|t| (t, provider_tag("litellm")));
                }
                bail!("LiteLLM selected but no valid API key")
            }
            "local_mt" => {
                #[cfg(feature = "local-mt")]
                return self.translate_local_mt(text, direction);
                #[cfg(not(feature = "local-mt"))]
                bail!("Local MT selected but binary was built without 'local-mt' feature")
            }
            _ => {} // "auto" — hedge all available providers, first success wins
        }

        // Auto mode: launch Groq immediately, LiteLLM and Yandex with staggered start.
        // Accept the first successful response.
        let has_groq = self.groq_key.len() > 4;
        let has_litellm = self.litellm_api_key.len() > 2 && !self.litellm_base_url.is_empty();
        let has_yandex = self.yandex_key.len() > 4;

        let total_tasks = has_groq as usize + has_litellm as usize + has_yandex as usize;
        if total_tasks == 0 {
            bail!("No valid API keys available for translation");
        }

        let (tx, rx) = std::sync::mpsc::channel::<(String, Result<String>)>();

        if has_groq {
            let tx = tx.clone();
            let s = self.clone();
            let t = text.to_string();
            let d = direction.clone();
            std::thread::spawn(move || {
                let _ = tx.send(("groq".into(), s.translate_groq(&t, &d)));
            });
        }
        if has_litellm {
            let tx = tx.clone();
            let s = self.clone();
            let t = text.to_string();
            let d = direction.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(200));
                let _ = tx.send(("litellm".into(), s.translate_litellm(&t, &d)));
            });
        }
        if has_yandex {
            let tx = tx.clone();
            let s = self.clone();
            let t = text.to_string();
            let d = direction.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                let _ = tx.send(("yandex".into(), s.translate_yandex_raw(&t, &d)));
            });
        }
        // Drop the original sender so rx.iter() terminates once all threads finish.
        drop(tx);

        let mut last_err = None;
        for (provider, result) in rx {
            match result {
                Ok(translated) => {
                    eprintln!("[AUTO] First response from {}: '{}'", provider, translated);
                    return Ok((translated, provider));
                }
                Err(e) => {
                    let err_line = format!("[FALLBACK] {} failed: {:#}", provider, e);
                    eprintln!("{}", err_line);
                    log_to_file("groq_debug.log", &err_line);
                    last_err = Some(e);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("All providers failed")))
    }

    #[cfg(feature = "local-mt")]
    fn translate_local_mt(&self, text: &str, direction: &TranslationDirection) -> Result<(String, String)> {
        let (cell, path) = if direction.from_code == "ru" && direction.to_code == "en" {
            (&self.mt_ruen, &self.mt_ruen_path)
        } else if direction.from_code == "en" && direction.to_code == "ru" {
            (&self.mt_enru, &self.mt_enru_path)
        } else {
            bail!("Local MT not available for {}→{}", direction.from_code, direction.to_code);
        };

        // An unconfigured or broken model is an ordinary translation error, not a panic
        // in the worker thread.
        let mt = match cell.get() {
            Some(loaded) => loaded,
            None => {
                if path.is_empty() {
                    bail!("Local MT model path not configured for {}→{}", direction.from_code, direction.to_code);
                }
                let loaded = model::TranslationModel::new(std::path::Path::new(path))
                    .with_context(|| format!("Failed to load local MT model from {}", path))?;
                let _ = cell.set(loaded);
                cell.get().context("Local MT model was not stored")?
            }
        };

        let result = mt.translate(text)?;
        eprintln!("[LOCAL_MT] {}→{}: '{}' → '{}'", direction.from_code, direction.to_code, text, result);
        Ok((result, "local_mt".into()))
    }

    fn translate_litellm(&self, text: &str, direction: &TranslationDirection) -> Result<String> {
        let translator = self.litellm.get_or_init(|| {
            litellm::LiteLlmTranslator::new(
                &self.litellm_base_url,
                &self.litellm_api_key,
                &self.litellm_model,
            ).expect("LiteLLM translator init failed")
        });
        translator.translate(text, &direction.from_code, &direction.to_code)
    }

    fn translate_groq(&self, text: &str, direction: &TranslationDirection) -> Result<String> {
        let system_prompt = groq_system_prompt(&direction.from_name, &direction.to_name);

        let model = self.groq_model.as_str();
        let body = groq_request_body(model, &system_prompt, text);
        let body_string = serde_json::to_string(&body).unwrap_or_default();

        // No key characters in the log: only the model and the text.
        let debug_line = format!("📡 GROQ req: model={}, text={}", model, text);
        eprintln!("{}", debug_line);
        log_to_file("groq_debug.log", &debug_line);

        // Retry loop: 3 attempts with exponential backoff (1s, 2s, 4s)
        let max_attempts = 3;
        let mut last_error = None;
        let mut last_was_transient_404 = false;
        for attempt in 1..=max_attempts {
            if attempt > 1 {
                // A transient 404 needs a new connection, not a long wait.
                let delay_ms = if last_was_transient_404 { 100 } else { 1000u64 * 2u64.pow(attempt as u32 - 2) };
                let retry_msg = format!("🔁 GROQ retry #{}/{} after {}ms", attempt, max_attempts, delay_ms);
                eprintln!("{}", retry_msg);
                log_to_file("groq_debug.log", &retry_msg);
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }

            let client = self.groq_client.lock().map(|c| c.clone()).unwrap_or_else(|_| self.client.clone());
            let response = client
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
                        .and_then(|c| c.message.content)
                        .map(|c| clean_groq_output(&c))
                        .unwrap_or_default();
                    if translated.trim().is_empty() {
                        // A reasoning model that ran out of tokens answers with nothing.
                        bail!("Groq returned an empty translation (model {})", model);
                    }


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
                    last_was_transient_404 = is_transient_not_found(status.as_u16(), &body_text);
                    if last_was_transient_404 {
                        // Drop the pooled connection: the next attempt (and later calls) start fresh.
                        if let (Ok(mut slot), Ok(fresh)) = (self.groq_client.lock(), new_http_client()) {
                            *slot = fresh;
                        }
                    } else if status.as_u16() >= 400 && status.as_u16() < 500 {
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

    fn translate_yandex(&self, text: &str, direction: &TranslationDirection) -> Result<(String, String)> {
        let result = self.translate_yandex_raw(text, direction)?;

        let out_line = format!("✅ YANDEX Output: {}", result);
        eprintln!("{}", out_line);
        log_to_file("groq_debug.log", &out_line);

        Ok((result, "yandex".into()))
    }

    fn translate_yandex_raw(&self, text: &str, direction: &TranslationDirection) -> Result<String> {
        let translator = self.yandex.get_or_init(|| {
            yandex::YandexTranslator::new(&self.yandex_key, &self.yandex_folder_id)
                .expect("Yandex translator init failed")
        });
        translator.translate(text, &direction.from_code, &direction.to_code)
    }
}

/// Tidy a model reply without ever rewriting what was translated. (The old version guessed at
/// "verbose" replies and cut real translations: anything containing "okay", "first," or a pair of
/// quotation marks lost words.)
fn clean_groq_output(text: &str) -> String {
    let mut t = text.to_string();

    // A reasoning model may leave a <think>...</think> block in the content.
    while let (Some(open), Some(close)) = (t.find("<think>"), t.find("</think>")) {
        if close < open {
            break;
        }
        t.replace_range(open..close + "</think>".len(), "");
    }
    let mut t = t.replace("<think>", "").replace("</think>", "");

    // Replies are single-line text for the speech synthesiser.
    t = t.split_whitespace().collect::<Vec<_>>().join(" ");

    // A leading label such as "Translation:" or "Перевод:".
    for label in ["translation:", "перевод:", "translated:"] {
        if t.to_lowercase().starts_with(label) {
            t = t.chars().skip(label.chars().count()).collect::<String>().trim().to_string();
            break;
        }
    }

    // One pair of quotes around the whole reply is formatting, not part of the translation.
    let pairs = [('"', '"'), ('\'', '\''), ('\u{ab}', '\u{bb}'), ('\u{201c}', '\u{201d}')];
    for (open, close) in pairs {
        if t.chars().count() >= 2 && t.starts_with(open) && t.ends_with(close) {
            let inner: String = t.chars().skip(1).take(t.chars().count() - 2).collect();
            if !inner.contains(open) && !inner.contains(close) {
                t = inner.trim().to_string();
            }
            break;
        }
    }

    // Typographic characters that the speech synthesiser reads wrongly or skips.
    t.chars()
        .map(|c| match c {
            '\u{2019}' | '\u{2018}' => '\'',
            '\u{2011}' | '\u{2010}' => '-',
            '\u{a0}' | '\u{202f}' | '\u{2009}' => ' ',
            other => other,
        })
        .collect::<String>()
        .trim()
        .to_string()
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
    #[serde(default)]
    content: Option<String>,
}

#[cfg(test)]
mod groq_tests {
    use super::*;

    #[test]
    fn reasoning_models_get_effort_and_a_larger_budget() {
        let body = groq_request_body("openai/gpt-oss-20b", "sys", "привет");
        assert_eq!(body["reasoning_effort"], "low");
        assert_eq!(body["max_tokens"], 400);
        assert_eq!(body["model"], "openai/gpt-oss-20b");
        assert_eq!(body["messages"][1]["content"], "привет");
    }

    #[test]
    fn other_models_keep_the_short_budget_and_no_reasoning_field() {
        let body = groq_request_body("llama-3.1-8b-instant", "sys", "hi");
        assert_eq!(body["max_tokens"], 40);
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn null_or_missing_content_does_not_break_parsing() {
        let null: GroqResponse =
            serde_json::from_str(r#"{"choices":[{"message":{"content":null,"reasoning":"x"}}]}"#).unwrap();
        assert!(null.choices[0].message.content.is_none());
        let ok: GroqResponse = serde_json::from_str(r#"{"choices":[{"message":{"content":"hello"}}]}"#).unwrap();
        assert_eq!(ok.choices[0].message.content.as_deref(), Some("hello"));
    }

    #[test]
    fn a_bare_not_found_is_transient_but_a_missing_model_is_not() {
        assert!(is_transient_not_found(404, r#"{"error":{"message":"Not Found"}}"#));
        assert!(!is_transient_not_found(
            404,
            r#"{"error":{"message":"The model `x` does not exist or you do not have access to it.","code":"model_not_found"}}"#
        ));
        assert!(!is_transient_not_found(401, r#"{"error":{"message":"Invalid API Key"}}"#));
        assert!(!is_transient_not_found(400, r#"{"error":{"message":"Not Found"}}"#));
    }

    #[test]
    fn prompt_names_both_languages_and_keeps_no_placeholders() {
        let prompt = groq_system_prompt("Russian", "English");
        assert!(prompt.contains("Russian to English"));
        assert!(!prompt.contains("{from}") && !prompt.contains("{to}"));
        assert!(!prompt.contains("word for word"), "the old prompt produced word-for-word grammar");
    }

    #[test]
    fn real_translations_are_not_rewritten() {
        // The old cleaner dropped "Okay." and cut text around quotation marks.
        assert_eq!(clean_groq_output("Okay. Let's meet at three."), "Okay. Let's meet at three.");
        assert_eq!(clean_groq_output("First, we need to check the numbers."), "First, we need to check the numbers.");
        assert_eq!(clean_groq_output(r#"He said "yes" and left."#), r#"He said "yes" and left."#);
        assert_eq!(clean_groq_output("Он сказал «да» и ушёл."), "Он сказал «да» и ушёл.");
    }

    #[test]
    fn formatting_around_the_reply_is_removed() {
        assert_eq!(clean_groq_output(r#""Good afternoon.""#), "Good afternoon.");
        assert_eq!(clean_groq_output("«Добрый день.»"), "Добрый день.");
        assert_eq!(clean_groq_output("Translation: Good afternoon."), "Good afternoon.");
        assert_eq!(clean_groq_output("Перевод: Добрый день."), "Добрый день.");
        assert_eq!(clean_groq_output("<think>hmm</think>Good afternoon."), "Good afternoon.");
        assert_eq!(clean_groq_output("  Good\n afternoon.  "), "Good afternoon.");
    }

    #[test]
    fn typography_the_voice_reads_wrongly_is_normalised() {
        assert_eq!(clean_groq_output("Let\u{2019}s go"), "Let's go");
        assert_eq!(clean_groq_output("бизнес\u{2011}приложения"), "бизнес-приложения");
        assert_eq!(clean_groq_output("в\u{a0}три часа"), "в три часа");
    }
}
