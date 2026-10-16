# Technology Stack

**Analysis Date:** 2026-10-16

## Languages

**Primary:**
- Rust (edition 2021) - Real-time audio engine, STT/translation/TTS clients, ONNX inference. Sources under `native/audio_engine/src/` (entry `native/audio_engine/src/main.rs`, pipeline `native/audio_engine/src/engine.rs`).
- Elixir (`~> 1.19`, declared in `mix.exs`) - OTP orchestrator that supervises the Rust engine as a Port and runs the TCP command server. Sources under `lib/translator/`.
- Python 3 - Flask web UI, settings/DB layer, Whisper worker subprocess. Sources in `web.py`, `web/`, `whisper_worker.py`, `whisper_stt.py`, `whisper_run.py`.

**Secondary:**
- JavaScript (vanilla, no build step) - Browser UI in `web/static/js/app.js`, `web/static/js/tooltips.js`.
- HTML/Jinja templates - `web/templates/index.html`, `web/templates/history.html`.
- CSS - `web/static/css/style.css`.
- PowerShell - Setup/run scripts: `setup.ps1`, `install-dependencies.ps1`, `run.ps1`, `rebuild.ps1`.
- Bash - `setup.sh`, `run.sh` (macOS-oriented paths such as Homebrew and BlackHole are present in `setup.sh`).
- XML - GRACE project docs in `docs/*.xml` (not runtime).

## Runtime

**Target platform:**
- Windows 10/11 is the primary target (WASAPI via cpal/wasapi, `pycaw`, VB-Audio Virtual Cable device names, `taskkill`/`netstat` calls in `lib/translator/command_server.ex`).
- macOS code paths exist (`native/audio_engine/src/tts/piper.rs` checks `/opt/homebrew/bin/espeak-ng`; `lib/translator/config` and `mix.exs` reference `/opt/homebrew/opt/rustup/bin`; `web/helpers.py` has `_list_audio_devices_macos`). Treat as secondary.

**Erlang/Elixir:**
- Elixir `~> 1.19` (`mix.exs`).
- Erlang/OTP version is not pinned in the repo. `setup.ps1` downloads Elixir 1.16.2 / OTP 26 (`setup.ps1`, line ~58), which conflicts with `mix.exs`. Check the installed version before building.

**Rust:**
- No `rust-toolchain` file. Toolchain is whatever `cargo` is on PATH (`mix.exs` `compile_rust/1` searches `~/.cargo/bin` first).
- Build profile: `cargo build --release` (invoked from `mix.exs` via the `compile.rust` alias).

**Python:**
- Python 3.12 expected. `native/audio_engine/src/stt/whisper_stt.rs` hardcodes `C:\Program Files\Python312\pythonw.exe` to spawn `whisper_worker.py`.
- `setup.ps1` creates `.venv` and installs `requirements.txt` into it.

**Package Managers:**
- Mix (Hex) - `mix.lock` present. Only one dependency locked: `jason 1.4.4`.
- Cargo - `native/audio_engine/Cargo.lock` present.
- pip - `requirements.txt` present (only two pins, see below). No lock file for Python.

## Frameworks

**Core:**
- Elixir OTP application (`Translator.Application` in `lib/translator/application.ex`) - `rest_for_one` supervisor managing `Translator.Config`, `Translator.AudioEngine`, `Translator.PipelineSupervisor`, `Translator.CommandServer`.
- Flask 3.1.3 - Web UI and REST/SSE endpoints (`web/__init__.py`, `web/routes.py`). Served on `127.0.0.1:5050` (`web.py`).
- cpal 0.15.3 - Cross-platform audio capture/playback (`native/audio_engine/src/audio/capture.rs`, `playback.rs`, `mod.rs`).
- wasapi 0.23.0 + windows 0.62 - WASAPI loopback and COM device enumeration (`native/audio_engine/src/audio/loopback.rs`).
- ort 2.0.0-rc.12 (feature `load-dynamic`) - ONNX Runtime bindings for Piper TTS and VAD (`native/audio_engine/src/tts/piper.rs`, `native/audio_engine/src/vad/mod.rs`).

**Testing:**
- No formal test framework. Rust `src/bin/test_*.rs` binaries act as manual probes (declared in `native/audio_engine/Cargo.toml` as `test_onnx_session` and `test_tts`; others are auto-discovered). `docs/technology.xml` states "No formal test suite".
- ExUnit not present (no `test/` directory in the tracked files).

**Build/Dev:**
- `mix` with custom compiler `:rust` (`mix.exs`) - Invokes cargo from Elixir.
- `build.rs` at `native/audio_engine/build.rs` - empty `fn main() {}`.
- `.formatter.exs` - Elixir formatter config.
- No ESLint/Prettier/bundler for the JS frontend.

## Key Dependencies

**Rust (`native/audio_engine/Cargo.toml`):**
- `tungstenite 0.21.0` (native-tls) - WebSocket client for Deepgram streaming STT (`native/audio_engine/src/stt/mod.rs`).
- `reqwest 0.12.15` (blocking, json, native-tls) - HTTP for Yandex STT/Translate, Groq, LiteLLM (`native/audio_engine/src/stt/yandex_stt.rs`, `native/audio_engine/src/translation/mod.rs`, `native/audio_engine/src/translation/litellm.rs`, `native/audio_engine/src/translation/yandex.rs`).
- `ort 2.0.0-rc.12` - ONNX Runtime; DLL loaded at runtime via `ORT_DYLIB_PATH`.
- `rubato 0.15.0` - Sinc resampling (`native/audio_engine/src/tts/piper.rs` `resample`).
- `ringbuf 0.4.8`, `crossbeam-channel 0.5.15` - Lock-free/thread channels in pipeline.
- `ndarray 0.17.2` - Tensor arrays for ONNX I/O.
- `serde 1.0.228`, `serde_json 1.0.149` - Protocol and API payloads (`native/audio_engine/src/protocol.rs`).
- `log 0.4.29`, `env_logger 0.11.9`, `anyhow 1.0.102` - Logging and errors. Custom trace file via `native/audio_engine/src/tracelog.rs`.
- `base64 0.22.1` - Audio encoding for payloads.
- Optional (Windows, only when feature `local-mt` is enabled): `ct2rs 0.10` - CTranslate2 local MT (`native/audio_engine/src/translation/model.rs`).
- Feature `directml` (empty flag) gates the DirectML GPU execution provider for Piper TTS in `native/audio_engine/src/tts/piper.rs` (`#[cfg(feature = "directml")]`, line ~89). Off by default: TTS runs on CPU.
- Known gap: `local-mt = []` in `Cargo.toml` does not activate `dep:ct2rs`, so enabling `local-mt` alone will not link ct2rs. Verify before using this feature.

**Elixir (`mix.exs`):**
- `jason 1.4.4` - JSON encode/decode for the Port protocol (`lib/translator/audio_engine.ex`).

**Python (`requirements.txt`):**
- `Flask==3.1.3` - Web framework.
- `pycaw==20251023` - Windows Core Audio device enumeration (`web/helpers.py` `_list_audio_devices_windows`, `webhelpers.py`).

**Python (used but NOT pinned in `requirements.txt`):**
- `faster-whisper` - Local Whisper STT in `whisper_worker.py`, `whisper_stt.py`, `whisper_run.py`. `docs/technology.xml` claims version 1.2.1; this is not enforced by any file.
- `numpy` - Imported by `whisper_stt.py` (line 9). Not pinned.
- Standard library only elsewhere (`sqlite3`, `urllib.request`, `subprocess`, `socket`).

**Native binaries committed to the repo:**
- `onnxruntime.dll` (~14 MB, tracked) and `onnxruntime.dll.old` (~11 MB, in `.gitignore`). The DLL is loaded by path from `ORT_DYLIB_PATH` (see INTEGRATIONS.md).
- `native/audio_engine/vendor/.gitkeep` - Vendor directory placeholder (no vendored code).
- Voxtral C FFI in `native/audio_engine/src/stt/voxtral.rs` (links `voxtral.c` symbols `vox_load`, `vox_stream_feed`, etc.) is NOT compiled: `native/audio_engine/src/stt/mod.rs` declares only `yandex_stt` and `whisper_stt`. Treat as dead code; no linker setup exists in `build.rs`.

## Models and Data Assets (not in git)

- Piper TTS voices (`*.onnx` + `*.onnx.json`) stored under a `models/` directory. Default layout: `models/piper-<lang>/<voice>.onnx`. Downloaded from HuggingFace (see INTEGRATIONS.md). Path resolution differs between components (see Configuration).
- Whisper model weights: fetched by `faster-whisper` on first use (name from env `WHISPER_MODEL`, default `tiny`).
- Silero VAD ONNX model loaded in `native/audio_engine/src/vad/mod.rs` (`commit_from_file`, line ~79); path is passed in by the caller.
- Local MT model directories (CTranslate2 `model.bin` + SentencePiece) only when feature `local-mt` is used (`native/audio_engine/src/translation/model.rs`).

## Configuration

**Runtime configuration sources (in precedence order as implemented):**
1. `settings.json` at repo root, written by the Flask UI (`web/settings.py` `SETTINGS_FILE`). Holds API keys in plaintext. Git-ignored.
2. Environment variables (`.env.example` lists the documented names; `.env` is git-ignored and was not read for this analysis).
3. Hardcoded defaults in `lib/translator/config.ex` and `web/settings.py` (`DEFAULT_SETTINGS`, `DEFAULT_VOICES`).

**Elixir side:**
- `config/config.exs` - Sets `audio_engine_path` (points to `native/audio_engine/target/release/audio_engine.exe`), `vad_threshold: 0.5`, `min_silence_ms: 500`, `transcript_timeout_ms: 2000`, `sample_rate: 48000`, and console logger format.
- `lib/translator/config.ex` - Agent-backed runtime config store (`Translator.Config`), with the same defaults (merges `Application` env over `@defaults`).
- `lib/translator/audio_engine.ex` `open_port/0` - Builds the environment passed to the engine process. It hardcodes absolute paths: `C:/Users/Pavel/AppData/Roaming/npm/call-translator-win` (models base and `ORT_DYLIB_PATH`). This must be changed before the project runs on any other machine.

**Rust engine:**
- Reads env vars in `native/audio_engine/src/engine.rs` (lines ~59-127) and `native/audio_engine/src/main.rs` (line ~55). Model path default: `./models` (`TRANSLATOR_MODELS_DIR`).
- `RUST_LOG` is set to `info` by the Elixir side.

**Python side:**
- `web/settings.py` - Constants: `CMD_PORT = 5051`, `CALL_IDLE_TIMEOUT = 300`, `GROQ_MODEL = "llama-3.3-70b-versatile"`, Groq/Deepgram/Piper URLs, `BASE_DIR`-relative `settings.json`, `calls.db`, `models/`, `test-log.txt`.
- `whisper_stt.py` / `whisper_worker.py` - `WHISPER_MODEL` (default `tiny`), `WHISPER_DEVICE` (default `cpu`), `WHISPER_COMPUTE` (default `int8`). `whisper_worker.py` hardcodes `device="cpu", compute_type="int8"` at line 64, so the env vars apply only to `whisper_stt.py`.
- `web/db.py` - SQLite file `calls.db` (`web/settings.py` `DB_FILE`). Tables `calls` and `utterances`.

**Build:**
- `mix.exs` - `compile.rust` alias runs `cargo build --release` in `native/audio_engine`; sets `MACOSX_DEPLOYMENT_TARGET=14.0` and prepends `~/.cargo/bin` and `/opt/homebrew/opt/rustup/bin` to PATH.
- `native/audio_engine/Cargo.toml` - Features: `default`, `directml`, `local-mt`.
- `.gitignore` - Excludes `native/audio_engine/target/`, `_build/`, `deps/`, `.venv/`, `settings.json`, `calls.db`, `*.log`, `.env`.
- No CI pipeline files (`.github/` contains only issue/PR templates).

## Platform Requirements

**Development (Windows, as used by setup scripts):**
- Elixir with Erlang/OTP (`setup.ps1` installs Elixir via GitHub installer).
- Rust via rustup (`setup.ps1` line ~99, `install-dependencies.ps1` line ~39). Cargo must be on PATH or in `%USERPROFILE%\.cargo\bin`.
- Python 3.12 at `C:\Program Files\Python312\` (hardcoded in `native/audio_engine/src/stt/whisper_stt.rs`).
- VB-Audio Virtual Cable (device names `CABLE Input` / `CABLE Output` are defaults in `web/settings.py` and `lib/translator/audio_engine.ex`).
- `espeak-ng` on PATH (required by Piper phonemization, `native/audio_engine/src/tts/piper.rs` `find_espeak_ng`).
- ONNX Runtime DLL reachable via `ORT_DYLIB_PATH`.

**Production:**
- Not a server deployment. Single-machine desktop app: Elixir node + Rust `audio_engine.exe` (spawned Port) + Flask on `127.0.0.1:5050`. Runs locally with API calls to external providers.

---

*Stack analysis: 2026-10-16*
