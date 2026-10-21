# External Integrations

**Analysis Date:** 2026-10-16

## APIs & External Services

**Speech-to-Text (STT):**

- Deepgram (default STT when `stt_provider` is `auto` or `deepgram`)
  - Protocol: WebSocket streaming, `wss://api.deepgram.com/v1/listen` with `model=nova-2-general`, `encoding=linear16`, `interim_results=true`, `endpointing`, `utterance_end_ms`, `vad_events=true`.
  - Implementation: `native/audio_engine/src/stt/mod.rs` (`DeepgramStt::create_session`, around lines 44-70). Auth header `Authorization` set at line ~83.
  - SDK/Client: `tungstenite 0.21.0` (native-tls), no Deepgram SDK.
  - Auth: env `DEEPGRAM_API_KEY` or `deepgram_api_key` in `settings.json`.
  - Key validation from UI: `web/routes.py` `POST /api/test-key` calls `GET https://api.deepgram.com/v1/projects` (URL constant `DEEPGRAM_API_URL` in `web/settings.py`) with `Authorization: Token <key>`.
  - Debug/test probe: `native/audio_engine/src/bin/test_ws.rs` (also contains a `wss://echo.websocket.events` probe URL).

- Yandex SpeechKit STT (when `stt_provider` is `yandex`, or as fallback under `auto` if Deepgram fails and a Yandex key is set)
  - Token exchange: `POST https://iam.api.cloud.yandex.net/iam/v1/tokens` with JSON body `yandexPassportOauthToken: <key>` (`native/audio_engine/src/stt/yandex_stt.rs`, `get_iam_token`, line ~43).
  - Recognition: `POST https://stt.api.cloud.yandex.net/speech/stt/v1/recognize?folderId=<id>&lang=<code>` (line ~94), header `Authorization: Api-Key <key>` (line ~102). Note the two different auth schemes in the same module.
  - Language mapping: `native/audio_engine/src/stt/mod.rs` (`pt` -> `pt-BR`, `no` -> `nb`, `ru` -> `ru-RU`, others passthrough; around lines 30-40, 88).
  - Auth env: `YANDEX_API_KEY`, `YANDEX_FOLDER_ID`.

- Whisper (local, `stt_provider = whisper` or `local_first` preset)
  - Engine spawns `whisper_worker.py` with `C:\Program Files\Python312\pythonw.exe` (hardcoded) from `native/audio_engine/src/stt/whisper_stt.rs` (`WhisperSttSession::new`, lines ~27-33; uses `CREATE_NO_WINDOW` via `std::os::windows::process::CommandExt`).
  - IPC: length-prefixed (4-byte big-endian) JSON over stdin/stdout. Init message `{"cmd":"init","model":...}`; transcribe `{"cmd":"transcribe","audio_b64":...}`. Protocol described in `whisper_worker.py` header (lines 1-6).
  - Library: `faster-whisper` (`WhisperModel`), model weights downloaded by faster-whisper from Hugging Face on first use (not explicitly configured in this repo).
  - Env: `WHISPER_MODEL` (default `tiny`), `WHISPER_DEVICE`, `WHISPER_COMPUTE` (read only by `whisper_stt.py`; `whisper_worker.py` hardcodes `cpu`/`int8`).
  - Note: `whisper_stt.rs` passes `WHISPER_MODEL` via the init message; `native/audio_engine/src/engine.rs` reads it from env with default `tiny`.

- Voxtral (`native/audio_engine/src/stt/voxtral.rs`) - Not compiled into the engine (module not declared in `stt/mod.rs`). No active integration.

**Translation:**

- Groq (Chat Completions, OpenAI-compatible)
  - Endpoint: `https://api.groq.com/openai/v1/chat/completions`.
  - Used by three components:
    - Rust translation (`native/audio_engine/src/translation/mod.rs` line ~314, model `llama-3.1-8b-instant` line ~276).
    - Python `web/helpers.py` `call_groq` and `webhelpers.py` `call_groq` (model `llama-3.3-70b-versatile`, constant `GROQ_MODEL` in `web/settings.py`).
    - Python summary endpoint `POST /api/calls/<id>/summary` in `web/routes.py` (line ~401, temperature 0.3, timeout 30s).
    - Key validation: `POST /api/test-key` (`web/routes.py` line ~85) sends a 1-token chat request; 401 maps to "Invalid API key".
  - SDK: none; raw `reqwest` (Rust) and `urllib.request` (Python).
  - Auth: `Authorization: Bearer <key>`. Env `GROQ_API_KEY` or `groq_api_key` in `settings.json`.
  - Python `web/settings.py` `get_groq_key()` falls back to env.

- Yandex Translate
  - Endpoint: `POST https://translate.api.cloud.yandex.net/translate/v2/translate`.
  - Rust: `native/audio_engine/src/translation/yandex.rs` (line ~47-48).
  - Python: `web/helpers.py` `call_yandex` (line ~44-45).
  - Body: `sourceLanguageCode`, `targetLanguageCode`, `texts`, `folderId`.
  - Auth: `Authorization: Api-Key <YANDEX_API_KEY>`, folder from `YANDEX_FOLDER_ID`.
  - Note: Yandex STT uses an IAM token minted from the same key; Translate uses the key directly as `Api-Key`. Both require the key to be valid for the same folder.

- LiteLLM / AI Portal (OpenAI-compatible gateway, user-hosted)
  - Client: `native/audio_engine/src/translation/litellm.rs` - `POST {base_url}/chat/completions`, `Authorization: Bearer <key>`, body `model`.
  - Env: `LITELLM_BASE_URL` (trailing slash trimmed), `LITELLM_API_KEY`, `LITELLM_MODEL` (default `ollama:ministral-3:3b-cloud`, `native/audio_engine/src/engine.rs` line ~93).
  - Not exposed in the Python UI settings; only reachable via the engine env.

- Local MT (CTranslate2 via `ct2rs`, feature `local-mt`)
  - `native/audio_engine/src/translation/model.rs` loads `model.bin` plus a SentencePiece tokenizer from a directory.
  - Paths: env `TRANSLATOR_LOCAL_MT_RUEN` and `TRANSLATOR_LOCAL_MT_ENRU`; settings `local_mt_ruen_path`, `local_mt_enru_path` (`web/settings.py` defaults).
  - Off unless the feature is enabled; see the `local-mt` feature caveat in `.planning/codebase/STACK.md`.

- Translation provider dispatch
  - Selected by `translation_provider` (`groq`, `yandex`, `litellm`, `local_mt`, or `auto`).
  - `auto` (`native/audio_engine/src/translation/mod.rs` lines ~155-200) fans out to all configured providers in parallel and takes the first success.
  - The Elixir side overrides both STT and translation to `whisper` / `local_mt` when `local_first` is true (`lib/translator/audio_engine.ex` `open_port/0`).

**Text-to-Speech (TTS):**

- Piper TTS (local ONNX inference, no network at runtime)
  - Engine: `native/audio_engine/src/tts/piper.rs` (ONNX via `ort`, phonemization via `espeak-ng` subprocess; `find_espeak_ng` candidates `espeak-ng`, `/opt/homebrew/bin/espeak-ng`, `/usr/local/bin/espeak-ng`, `/usr/bin/espeak-ng`).
  - Model files: `<models_base>/piper-<lang>/<voice>.onnx` and `.onnx.json`.
  - Env: `TRANSLATOR_TTS_EN_MODEL`, `TRANSLATOR_TTS_EN_CONFIG`, `TRANSLATOR_TTS_RU_MODEL`, `TRANSLATOR_TTS_RU_CONFIG`, and `TRANSLATOR_MODELS_DIR` (default `./models`, `native/audio_engine/src/engine.rs` lines ~59, 98-105, 235-238).
  - Voice downloads (network, from UI): Hugging Face `https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0` (constant `PIPER_VOICES_URL`, `web/settings.py`). Catalog: `<base>/voices.json`. Files downloaded via `web/routes.py` `POST /api/download-voice` (`urllib.request`, timeout 120 s, chunked write; line ~200-212). Catalog fetch in `web/helpers.py` `get_voice_catalog` (line ~62-80) and `webhelpers.py`.
  - Default voices: `DEFAULT_VOICES` in `web/settings.py`.

- ONNX Runtime
  - Loaded dynamically (`ort` feature `load-dynamic`). `ort::init().commit()` called in `native/audio_engine/src/engine.rs` (line ~719) and `native/audio_engine/src/main.rs` (comment line ~67).
  - DLL path env: `ORT_DYLIB_PATH`. Repo ships `onnxruntime.dll` at repo root (tracked, ~14 MB).

**Audio devices (OS integration, not network):**

- WASAPI via `cpal` (`native/audio_engine/src/audio/*.rs`) and `wasapi` loopback (`native/audio_engine/src/audio/loopback.rs`).
- Python device enumeration: `pycaw` (`web/helpers.py` `_list_audio_devices_windows` line ~166; `webhelpers.py` line ~130). On macOS `_list_audio_devices_macos` in `web/helpers.py`.
- Virtual cable: VB-Audio Virtual Cable device names are the defaults `CABLE Output (VB-Audio Virtual Cable)` / `CABLE Input (VB-Audio Virtual Cable)` (`web/settings.py`, `lib/translator/audio_engine.ex`). Device env: `TRANSLATOR_MIC_DEVICE`, `TRANSLATOR_SPEAKER_DEVICE`, `TRANSLATOR_MEET_INPUT`, `TRANSLATOR_MEET_OUTPUT`.

**Model/tool downloads in setup scripts only (not runtime):**

- `setup.sh` - Piper catalog and voice download from Hugging Face (lines ~92-137); `pip install -r requirements.txt`; BlackHole URL `https://existential.audio/blackhole/` (macOS).
- `setup.ps1` - Elixir installer `https://github.com/elixir-lang/elixir/releases/download/v1.16.2/elixir-otp-26-windows-x64.exe` (line 58); rustup `https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe` (line 99); `.venv` pip install (line 139).
- `install-dependencies.ps1` - Chocolatey install script from `https://community.chocolatey.org/install.ps1` (line 15); rustup (line 39).

## Data Storage

**Databases:**

- SQLite (embedded, Python stdlib `sqlite3`)
  - File: `calls.db` at repo root (`DB_FILE` in `web/settings.py`), git-ignored.
  - Access: `web/db.py` (`_get_db`, `_ensure_call`, `_record_line`, `_close_call`, `_call_lock` threading lock). Idle auto-close after `CALL_IDLE_TIMEOUT = 300` s.
  - Schema: `CREATE TABLE IF NOT EXISTS calls` (line ~26) and `utterances` (line ~34) in `web/db.py`.
  - No ORM. Raw SQL with `?` parameters.

**File Storage:**

- Local filesystem only.
  - `settings.json` at repo root (`SETTINGS_FILE`, `web/settings.py`) - stores API keys in plaintext. Git-ignored.
  - `models/` at repo root (`MODELS_DIR`) - Piper voices and local MT models.
  - `pipeline_trace.log` - Written by `native/audio_engine/src/tracelog.rs` (truncated on start; `*.log` older than 24 h removed from working dir).
  - `test-log.txt` (`LOG_FILE`, `web/settings.py`), `deepgram_debug.log` (`native/audio_engine/src/stt/mod.rs` line ~17). `groq_debug.log` (`native/audio_engine/src/translation/mod.rs`, Groq request trace; git-ignored).

**Caching:**

- None (no Redis/memcached). In-process caches only: Python `_voice_catalog` global in `web/helpers.py`; Elixir `Translator.Config` Agent.

## Inter-Process Communication (local)

These are not external services but are the main integration seams.

- Elixir <-> Rust engine: Erlang Port, `{:spawn_executable, audio_engine.exe}` with `{:packet, 4}` framing and JSON payloads. Commands: `start`, `stop`, `ping`, `set_config`, plus engine-to-Elixir events. Implemented in `lib/translator/audio_engine.ex` (`open_port/0` line ~246; `send_to_port/2` line ~385). Protocol types in `native/audio_engine/src/protocol.rs` (serde tag `cmd`, snake_case).
  - Engine binary path: `config/config.exs` `audio_engine_path` -> `native/audio_engine/target/release/audio_engine.exe`.
  - Note: `open_port/0` passes `ORT_DYLIB_PATH` and the models base path as hardcoded `C:/Users/Pavel/AppData/Roaming/npm/call-translator-win/...` strings (see Environment below).

- Python UI <-> Elixir: TCP on `127.0.0.1:5051`, line-delimited JSON. Elixir server `lib/translator/command_server.ex` (`@port 5051`); Python client `send_engine_command` in `web/helpers.py` (line ~36) and `webhelpers.py` (line ~36), used by `POST /cmd` and `/api/engine/restart` in `web/routes.py`. On bind conflict, `command_server.ex` runs `netstat -ano` and `taskkill /F /PID` to kill the owner (Windows only).

- Rust engine <-> Python Whisper worker: stdin/stdout pipes, 4-byte length-prefixed JSON (see Whisper above).

- Browser <-> Flask: HTTP on `127.0.0.1:5050` (`web.py`). Routes: `/`, `/health`, `/api/settings` (GET/POST), `/api/test-key`, `/api/voices`, `/api/devices`, `/api/tts-preview`, `/api/download-voice`, `/api/engine/restart`, `/api/poll-audio`, `/api/translate`, `/api/calls/*`, `/history`, `/cmd`, `/stream` (Server-Sent Events) - defined in `web/routes.py`. `/api/translate` calls Yandex or Groq (`web/routes.py` line ~250-305).

## Authentication & Identity

- No user authentication. The Flask UI and TCP command server bind to `127.0.0.1` only.
- Third-party auth is API-key based:
  - Deepgram: `Token <key>` header.
  - Groq and LiteLLM: `Bearer <key>` header.
  - Yandex Translate: `Api-Key <key>`.
  - Yandex STT: IAM token minted from `yandexPassportOauthToken` (the key) at `iam.api.cloud.yandex.net`, then `Api-Key` on recognize call.
- Key resolution order: `settings.json` value first, then environment variable (`web/settings.py` `load_settings`, `get_groq_key`; `web/helpers.py` `get_yandex_key` line ~59-67; `lib/translator/audio_engine.ex` `charlist_setting/3`). On first launch with no `settings.json`, keys are copied from env.

## Monitoring & Observability

- Error tracking: None (no Sentry or similar).
- Logs: Python `logging` (stderr, configured in `web/__init__.py` and `web.py`); Elixir `Logger` with console format in `config/config.exs`; Rust `log`/`env_logger` plus `eprintln!` and `tracelog` file output (`pipeline_trace.log`). `RUST_LOG=info` set by the Elixir side when spawning the engine.
- Debug logs written to CWD (`deepgram_debug.log`, `groq_debug.log`, `pipeline_trace.log`) - these may contain transcripts; all are git-ignored.

## CI/CD & Deployment

- Hosting: None. Local Windows desktop process set started by `run.ps1` / `run.sh`.
- CI pipeline: None detected (`.github/` contains only issue and PR templates).

## Environment Configuration

**Environment variable names referenced by code (values never recorded here):**

- Provider keys: `DEEPGRAM_API_KEY`, `GROQ_API_KEY`, `YANDEX_API_KEY`, `YANDEX_FOLDER_ID`, `LITELLM_API_KEY`, `LITELLM_BASE_URL`, `LITELLM_MODEL`.
- Provider selection: `STT_PROVIDER`, `TRANSLATOR_PROVIDER`.
- Whisper: `WHISPER_MODEL`, `WHISPER_DEVICE`, `WHISPER_COMPUTE`.
- Local MT: `TRANSLATOR_LOCAL_MT_RUEN`, `TRANSLATOR_LOCAL_MT_ENRU`.
- Models and runtime: `TRANSLATOR_MODELS_DIR`, `TRANSLATOR_TTS_EN_MODEL`, `TRANSLATOR_TTS_EN_CONFIG`, `TRANSLATOR_TTS_RU_MODEL`, `TRANSLATOR_TTS_RU_CONFIG`, `ORT_DYLIB_PATH`, `RUST_LOG`.
- Audio devices: `TRANSLATOR_MIC_DEVICE`, `TRANSLATOR_SPEAKER_DEVICE`, `TRANSLATOR_MEET_INPUT`, `TRANSLATOR_MEET_OUTPUT`, `TRANSLATOR_SAMPLE_RATE`.
- Timing and languages: `TRANSLATOR_ENDPOINTING_MS`, `TRANSLATOR_UTTERANCE_END_MS`, `TRANSLATOR_MY_LANG`, `TRANSLATOR_THEIR_LANG`.
- Documented template: `.env.example` (root) lists `DEEPGRAM_API_KEY`, `GROQ_API_KEY`, `ORT_DYLIB_PATH`, `TRANSLATOR_MIC_DEVICE`, `TRANSLATOR_SPEAKER_DEVICE`, `TRANSLATOR_MEET_INPUT`, `TRANSLATOR_MEET_OUTPUT`, `YANDEX_API_KEY`, `YANDEX_FOLDER_ID`. The `.env` file itself was not read.

**Settings storage:**

- Runtime UI-managed settings: `settings.json` at repo root (plaintext keys; git-ignored). Read/written by `web/settings.py` (`load_settings`, `save_settings_to_file`).
- Env-based configuration is only applied as a fallback when a setting is empty.

**Hardcoded machine-specific values (must be fixed for portability):**

- `lib/translator/audio_engine.ex` `open_port/0`: `base_dir = "C:/Users/Pavel/AppData/Roaming/npm/call-translator-win"`, and the `ORT_DYLIB_PATH` value uses the same base. Models base also derives from it.
- `native/audio_engine/src/stt/whisper_stt.rs`: `C:\Program Files\Python312\pythonw.exe`.
- `native/audio_engine/src/engine.rs`: default models directory `./models` (relative to CWD), which differs from the Elixir value above.
- `native/audio_engine/src/tts/piper.rs`: espeak-ng candidate paths include macOS Homebrew locations.

**Secrets location:**

- Runtime keys: `settings.json` (plaintext) and/or process environment variables set by the Elixir Port spawn.
- `.env` (git-ignored) is the documented place for env values. `.env.example` contains placeholders only. The `.env` file was not read.

## Webhooks & Callbacks

**Incoming:**

- None. No inbound webhook endpoints from external services. Flask routes are local-only (127.0.0.1:5050) and the TCP command server is local-only (127.0.0.1:5051).

**Outgoing:**

- Deepgram WebSocket streaming (audio up, transcripts down) - `native/audio_engine/src/stt/mod.rs`.
- Yandex STT/Translate and IAM token exchange - `native/audio_engine/src/stt/yandex_stt.rs`, `native/audio_engine/src/translation/yandex.rs`, `web/helpers.py`.
- Groq chat completions - `native/audio_engine/src/translation/mod.rs`, `web/helpers.py`, `web/routes.py`.
- LiteLLM gateway chat completions - `native/audio_engine/src/translation/litellm.rs`.
- Hugging Face Piper voice catalog and file downloads - `web/helpers.py`, `web/routes.py`.

---

*Integration audit: 2026-10-16*
