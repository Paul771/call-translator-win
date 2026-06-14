# Yandex Translate Integration Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use compose:subagent (recommended) or compose:execute to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Yandex Translate API as an alternative translation provider, with automatic fallback from Groq to Yandex when Groq is unavailable (e.g., blocked from Russia).

**Architecture:** Modify the Rust `TranslationEngine` to support multiple providers. Add Yandex Translate REST API as a second backend. The engine tries Groq first; if it returns 403 or fails, it falls back to Yandex. Add `YANDEX_API_KEY` environment variable and settings support.

**Tech Stack:** Rust (reqwest), Elixir (env vars), Flask (settings UI)

---

## File Structure

| File | Action | Purpose |
|------|--------|---------|
| `native/audio_engine/src/translation/mod.rs` | Modify | Add Yandex provider, fallback logic |
| `native/audio_engine/src/translation/yandex.rs` | Create | Yandex Translate API client |
| `lib/translator/audio_engine.ex` | Modify | Pass YANDEX_API_KEY env var |
| `web/settings.py` | Modify | Add yandex_api_key to defaults |
| `web/routes.py` | Modify | Test Yandex key endpoint |
| `settings.json` | Modify | Add yandex_api_key field |
| `.env` | Modify | Add YANDEX_API_KEY placeholder |

---

## Task 1: Create Yandex Translate API Client

**Files:**
- Create: `native/audio_engine/src/translation/yandex.rs`

- [ ] **Step 1: Create the yandex.rs module**

```rust
use anyhow::{Context, Result};
use serde::Deserialize;

pub struct YandexTranslator {
    api_key: String,
    folder_id: String,
    client: reqwest::blocking::Client,
}

#[derive(Deserialize)]
struct YandexResponse {
    translations: Vec<YandexTranslation>,
}

#[derive(Deserialize)]
struct YandexTranslation {
    text: String,
}

impl YandexTranslator {
    pub fn new(api_key: &str, folder_id: &str) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .context("Failed to build HTTP client for Yandex")?;
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

        let body = serde_json::json!({
            "sourceLanguageCode": source_lang,
            "targetLanguageCode": target_lang,
            "texts": [text],
            "folderId": self.folder_id,
        });

        let response = self.client
            .post("https://translate.api.cloud.yandex.net/translate/v2/translate")
            .header("Authorization", format!("Api-Key {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&body).unwrap_or_default())
            .send()
            .context("Yandex Translate request failed")?;

        let status = response.status();
        let bytes = response.bytes().context("Failed to read Yandex response")?;
        let response_text = String::from_utf8(bytes.to_vec()).unwrap_or_default();

        if !status.is_success() {
            anyhow::bail!("Yandex Translate error {}: {}", status, response_text);
        }

        let yandex_resp: YandexResponse = serde_json::from_str(&response_text)
            .context("Failed to parse Yandex response")?;

        let translated = yandex_resp.translations
            .into_iter()
            .next()
            .map(|t| t.text)
            .unwrap_or_default();

        Ok(translated)
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd native/audio_engine && cargo check --lib 2>&1 | head -20`
Expected: No errors (warnings OK)

---

## Task 2: Integrate Yandex into TranslationEngine

**Files:**
- Modify: `native/audio_engine/src/translation/mod.rs`

- [ ] **Step 1: Add yandex module and update TranslationEngine**

Replace the entire `translation/mod.rs` with:

```rust
pub mod yandex;

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

pub enum TranslationProvider {
    Groq,
    Yandex,
}

#[derive(Clone)]
pub struct TranslationEngine {
    groq_key: String,
    yandex_key: String,
    yandex_folder_id: String,
    client: reqwest::blocking::Client,
}

impl TranslationEngine {
    pub fn new(groq_key: &str, yandex_key: &str, yandex_folder_id: &str) -> Result<Self> {
        eprintln!("[TRANSLATION] Providers: Groq={}, Yandex={}",
            if groq_key.len() >= 4 { &groq_key[..4] } else { "none" },
            if yandex_key.len() >= 4 { &yandex_key[..4] } else { "none" });

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
            client,
        })
    }

    pub fn translate(&self, text: &str, direction: &TranslationDirection) -> Result<String> {
        if text.trim().is_empty() {
            return Ok(String::new());
        }

        // Try Groq first if key is available
        if !self.groq_key.is_empty() && self.groq_key.len() > 4 {
            match self.translate_groq(text, direction) {
                Ok(result) => return Ok(result),
                Err(e) => {
                    eprintln!("[TRANSLATION] Groq failed: {}, trying Yandex...", e);
                    log_to_file("groq_debug.log", &format!("Groq failed: {}, falling back to Yandex", e));
                }
            }
        }

        // Fall back to Yandex if key is available
        if !self.yandex_key.is_empty() && self.yandex_key.len() > 4 {
            return self.translate_yandex(text, direction);
        }

        bail!("No translation provider available. Set GROQ_API_KEY or YANDEX_API_KEY.")
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
            "🔑 GROQ KEY={}...{} (len={})\n📡 req: model={}, text={}",
            &self.groq_key[..4.min(self.groq_key.len())],
            &self.groq_key[32..36.min(self.groq_key.len())],
            self.groq_key.len(),
            model,
            text,
        );
        eprintln!("{}", debug_line);
        log_to_file("groq_debug.log", &debug_line);

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
                    let text = String::from_utf8(bytes.to_vec()).unwrap_or_default();
                    let err_line = format!("❌ GROQ error {}: {}", status, text);
                    eprintln!("{}", err_line);
                    log_to_file("groq_debug.log", &err_line);
                    if status.as_u16() >= 400 && status.as_u16() < 500 {
                        bail!("Groq error {}: {}", status, text);
                    }
                    last_error = Some(anyhow::anyhow!("Groq error {}: {}", status, text));
                }
                Err(e) => {
                    let err_line = format!("❌ GROQ request failed: {:#}", e);
                    eprintln!("{}", err_line);
                    log_to_file("groq_debug.log", &err_line);
                    last_error = Some(anyhow::anyhow!("{}", e));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("GROQ all retries exhausted")))
    }

    fn translate_yandex(&self, text: &str, direction: &TranslationDirection) -> Result<String> {
        let translator = yandex::YandexTranslator::new(
            &self.yandex_key,
            &self.yandex_folder_id,
        )?;

        let debug_line = format!(
            "🔑 YANDEX KEY={}... (len={})\n📡 req: {}→{}, text={}",
            &self.yandex_key[..4.min(self.yandex_key.len())],
            self.yandex_key.len(),
            direction.from_code,
            direction.to_code,
            text,
        );
        eprintln!("{}", debug_line);
        log_to_file("groq_debug.log", &debug_line);

        let translated = translator.translate(text, &direction.from_code, &direction.to_code)?;

        let out_line = format!("✅ YANDEX Output: {}", translated);
        eprintln!("{}", out_line);
        log_to_file("groq_debug.log", &out_line);

        Ok(translated)
    }
}

fn clean_groq_output(text: &str) -> String {
    let t = text.trim();
    if let Some(pos) = t.to_lowercase().find("translates to") {
        let after = &t[pos + "translates to".len()..];
        if let Some(start) = after.find('"') {
            let rest = &after[start + 1..];
            if let Some(end) = rest.find('"') {
                return rest[..end].to_string();
            }
        }
        let first_sentence = after.split(|c| c == '.' || c == '!' || c == '?')
            .next().unwrap_or("").trim().trim_matches('"');
        if !first_sentence.is_empty() {
            return first_sentence.to_string();
        }
    }
    let parts: Vec<&str> = t.split('"').collect();
    if parts.len() >= 3 {
        if let Some(last) = parts.last() {
            let candidate = last.trim();
            if !candidate.is_empty() && candidate.len() < 100 {
                return candidate.to_string();
            }
        }
    }
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
```

- [ ] **Step 2: Verify it compiles**

Run: `cd native/audio_engine && cargo check --lib 2>&1 | head -20`
Expected: No errors

---

## Task 3: Update Elixir to Pass Yandex Keys

**Files:**
- Modify: `lib/translator/audio_engine.ex:279`

- [ ] **Step 1: Add YANDEX_API_KEY and YANDEX_FOLDER_ID env vars**

Find the `{:env, [...]}` block in `audio_engine.ex` and add after the GROQ_API_KEY line:

```elixir
{~c"YANDEX_API_KEY", charlist_setting(settings, "yandex_api_key", "YANDEX_API_KEY")},
{~c"YANDEX_FOLDER_ID", charlist_setting(settings, "yandex_folder_id", "YANDEX_FOLDER_ID")},
```

- [ ] **Step 2: Update Rust engine.rs to read the new env vars**

In `native/audio_engine/src/engine.rs`, find where `groq_api_key` is read and add:

```rust
let yandex_key = std::env::var("YANDEX_API_KEY").unwrap_or_default();
let yandex_folder_id = std::env::var("YANDEX_FOLDER_ID").unwrap_or_default();
```

Then update the `TranslationEngine::new()` call to pass both keys:

```rust
TranslationEngine::new(&self.config.groq_api_key, &yandex_key, &yandex_folder_id)
```

---

## Task 4: Update Settings and UI

**Files:**
- Modify: `web/settings.py`
- Modify: `settings.json`

- [ ] **Step 1: Add yandex_api_key to DEFAULT_SETTINGS in settings.py**

```python
DEFAULT_SETTINGS = {
    "deepgram_api_key": "",
    "groq_api_key": "",
    "yandex_api_key": "",
    "yandex_folder_id": "",
    # ... rest unchanged
}
```

- [ ] **Step 2: Add keys to settings.json**

```json
{
  "deepgram_api_key": "...",
  "groq_api_key": "...",
  "yandex_api_key": "",
  "yandex_folder_id": "",
  ...
}
```

---

## Task 5: Test the Integration

- [ ] **Step 1: Compile Rust engine**

Run: `cd native/audio_engine && cargo build --release 2>&1 | tail -5`
Expected: `Finished release profile`

- [ ] **Step 2: Compile Elixir**

Run: `mix compile 2>&1 | tail -5`
Expected: `Generated translator app`

- [ ] **Step 3: Start the app and verify engine starts**

Run the app, check logs for `[TRANSLATION] Providers: Groq=..., Yandex=...`

---

## Verification Checklist

1. [ ] Rust compiles without errors
2. [ ] Elixir compiles without errors
3. [ ] Engine starts and shows both providers in logs
4. [ ] If Groq key is invalid, falls back to Yandex automatically
5. [ ] Translation works bidirectionally (ru→en, en→ru)
