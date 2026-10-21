# Codebase Structure

**Analysis Date:** 2026-10-16

## Directory Layout

```
call-translator-win/                       # repo root (git worktree)
├── lib/translator/                        # Elixir OTP app: engine supervision, TCP command server
├── native/audio_engine/                   # Rust audio engine (Cargo crate, produces audio_engine.exe)
│   ├── src/                               # Engine sources (main.rs, engine.rs, protocol.rs, ...)
│   │   ├── audio/                         # WASAPI capture, loopback, playback, device enumeration
│   │   ├── pipeline/                      # Job queue and text segmenter
│   │   ├── stt/                           # Deepgram (in mod.rs), Yandex, Whisper, Voxtral
│   │   ├── translation/                   # Provider chain: Groq, LiteLLM, Yandex
│   │   ├── tts/                           # Piper TTS wrapper
│   │   ├── vad/                           # Voice activity detection
│   │   └── bin/                           # Diagnostic executables (test_*.rs, list_*.rs, check_env2.rs)
│   └── vendor/                            # Placeholder (.gitkeep) for vendored crates
├── web/                                   # Flask package used by web.py
│   ├── routes.py                          # HTTP routes, SSE /stream, /api/*
│   ├── helpers.py                         # Groq/Yandex HTTP, engine TCP client, voice catalog, devices
│   ├── db.py                              # SQLite calls.db access and log recording
│   ├── settings.py                        # Paths, defaults, load/save settings.json
│   ├── templates/                         # index.html (settings + live view), history.html
│   └── static/                            # js/app.js, js/tooltips.js, css/style.css
├── docs/                                  # GRACE project docs (XML plans, knowledge graph, backlogs)
├── config/config.exs                      # Elixir app config (audio_engine_path, etc.)
├── web.py                                 # Flask entry point (port 5050)
├── webhelpers.py                          # Older, smaller copy of web/helpers.py (not imported by package)
├── whisper_worker.py                      # Python Whisper worker spawned by Rust whisper_stt.rs
├── whisper_run.py, whisper_stt.py         # Standalone Whisper helpers (not referenced by runtime)
├── call-translator-win-git/               # Nested directory with no tracked files (see Notes)
├── onnxruntime.dll, onnxruntime.dll.old  # ONNX Runtime binaries loaded at runtime (load-dynamic)
├── run.ps1, run.sh                        # Launch scripts (Flask + Elixir + engine)
├── setup.ps1, setup.sh, install-dependencies.ps1, rebuild.ps1  # Setup/build scripts
├── requirements.txt                       # Python dependencies
├── mix.exs, mix.lock                      # Elixir project definition and lock
├── .env.example                           # Expected environment variable names
├── AGENTS.md                              # GRACE engineering protocol for this repo
├── README.md, README.ru.md, USAGE.md, WINDOWS_GUIDE.md, CHANGELOG.md
└── .github/                               # Issue and PR templates
```

Runtime-only paths (not tracked, created on use):
- `settings.json`, `calls.db`, `test-log.txt`, `models/` (repo root, see `web/settings.py`)
- `deepgram_debug.log` (written by `audio/mod.rs` device listing)
- `_build/`, `deps/`, `native/audio_engine/target/` (build output)

## Directory Purposes

**`lib/translator/`:**
- Purpose: Elixir OTP application for supervision and the Rust engine bridge.
- Contains: `application.ex` (supervision tree), `audio_engine.ex` (Port GenServer, event dispatch, 577 lines), `command_server.ex` (TCP 5051), `pipeline.ex` and `pipeline_supervisor.ex` (per-direction history), `config.ex` (app env defaults).
- Key files: `lib/translator/audio_engine.ex`, `lib/translator/command_server.ex`
- Top-level module: `lib/translator.ex`

**`native/audio_engine/src/`:**
- Purpose: Rust engine executable.
- Contains: Entry and orchestration (`main.rs`, `engine.rs` at 1328 lines, `protocol.rs`, `tracelog.rs`).
- Key files: `native/audio_engine/src/engine.rs` (pipeline spawn/run, TTS/translation glue), `native/audio_engine/src/protocol.rs` (`Command` and `Event` enums).

**`native/audio_engine/src/audio/`:**
- Purpose: Device I/O.
- Contains: `capture.rs` (mic capture via cpal), `loopback.rs` (WASAPI loopback, currently unused by pipelines), `playback.rs` (output stream), `mod.rs` (`list_devices`).

**`native/audio_engine/src/stt/`:**
- Purpose: Speech-to-text providers.
- Contains: `mod.rs` (Deepgram WebSocket session and `UnifiedSttSession` enum, 360 lines), `yandex_stt.rs`, `whisper_stt.rs` (spawns `whisper_worker.py`), `voxtral.rs` (not part of `UnifiedSttSession`).

**`native/audio_engine/src/translation/`:**
- Purpose: Text translation providers and selection.
- Contains: `mod.rs` (`TranslationEngine`, `TranslationDirection`, provider chain), `litellm.rs`, `yandex.rs`, `model.rs` (local model handling).

**`native/audio_engine/src/tts/`:**
- Purpose: Text-to-speech.
- Contains: `mod.rs` (`TtsEngine` wrapper), `piper.rs` (ONNX + espeak-ng).

**`native/audio_engine/src/pipeline/`:**
- Purpose: Job control and text delta computation for a direction.
- Contains: `job_queue.rs`, `segmenter.rs`, `mod.rs` (re-exports).

**`native/audio_engine/src/vad/`:**
- Purpose: Voice activity detection used for segmenting audio before STT.
- Contains: `mod.rs` (313 lines). Compiled as a module of the crate but not referenced by `engine.rs` or any other source file, so it is not on the runtime path.

**`native/audio_engine/src/bin/`:**
- Purpose: Standalone diagnostic binaries for devices, ONNX, WASAPI, and TTS. Not part of the runtime path.
- Contains: 20 `.rs` files, including `test_capture*.rs`, `test_tts.rs`, `test_onnx*.rs`, `test_ws.rs`, `list_devices_utf8.rs`.
- Declared `[[bin]]` entries in `native/audio_engine/Cargo.toml` (`test_onnx_session`, `test_tts`, ...).

**`web/`:**
- Purpose: Flask package.
- Contains: `routes.py` (registered by `register_routes(app)` from `web/__init__.py`), `helpers.py`, `db.py`, `settings.py`, `templates/`, `static/`.

**`web/static/js/`:**
- Purpose: Browser UI logic.
- Contains: `app.js` (EventSource on `/stream`, `/cmd` calls, `/api/poll-audio` playback loop), `tooltips.js`.

**`docs/`:**
- Purpose: GRACE project protocol documents (`requirements.xml`, `technology.xml`, `development-plan.xml`, `verification-plan.xml`, `knowledge-graph.xml`, `operational-packets.xml`), latency/backlog plans (`pipeline-latency-fix-plan.xml`, `w1-`/`w2-`/`w3-implementation-backlog.xml`, `deferred-fixes-backlog.xml`), and one compose plan under `docs/compose/plans/`.
- Note: `docs/knowledge-graph.xml` is the project map that AGENTS.md says must stay current.

## Key File Locations

**Entry Points:**
- `web.py`: Flask server start (port 5050).
- `lib/translator/application.ex`: OTP application and supervision tree.
- `native/audio_engine/src/main.rs`: Rust engine `main`, stdin/stdout loop.

**Configuration:**
- `config/config.exs`: Elixir application config (`:audio_engine_path`, etc.).
- `lib/translator/config.ex`: runtime config store (`Translator.Config`).
- `web/settings.py`: Python defaults (`DEFAULT_SETTINGS`), paths (`SETTINGS_FILE`, `LOG_FILE`, `DB_FILE`, `MODELS_DIR`), and `CMD_HOST`/`CMD_PORT`.
- `native/audio_engine/src/engine.rs`: `EngineConfig::from_env()` (line 58) defines the env variable contract.
- `.env.example`: Documented env names.
- `mix.exs`, `native/audio_engine/Cargo.toml`, `requirements.txt`: Dependency manifests.

**Core Logic:**
- `native/audio_engine/src/engine.rs`: Pipeline start/stop (`start_pipelines`, line 335), per-direction spawn (`spawn_pipeline`, line 464), main loop (`run_pipeline`, line 536), TTS and translation glue (around lines 1120-1300).
- `lib/translator/audio_engine.ex`: Port lifecycle and event dispatch (`dispatch_event/2`).
- `lib/translator/command_server.ex`: TCP command mapping.
- `web/routes.py`: All HTTP and SSE routes.

**Testing:**
- No `test/` directory is tracked. Rust diagnostics live in `native/audio_engine/src/bin/test_*.rs`. No Elixir or Python test suites found in the tracked tree.

## Naming Conventions

**Files:**
- Elixir: `snake_case.ex`, one module per file, path mirrors module (`lib/translator/audio_engine.ex` -> `Translator.AudioEngine`).
- Rust: `snake_case.rs`, module directories use `mod.rs` (for example `stt/mod.rs`, `translation/mod.rs`). Vendor-specific backends use a provider prefix: `yandex_stt.rs`, `whisper_stt.rs`, `litellm.rs`, `yandex.rs`.
- Python: `snake_case.py`. Package modules inside `web/` use short nouns (`routes`, `helpers`, `db`, `settings`).
- Diagnostic Rust binaries use `test_*.rs` or `list_*.rs` in `src/bin/`.

**Directories:**
- Lowercase, single-word or short (`stt`, `tts`, `audio`, `translation`, `pipeline`, `vad`). Exception: `native/audio_engine/` (underscore).

**Identifiers:**
- Pipeline direction names are the literal strings `"outgoing"` and `"incoming"` in Elixir, Rust, and the TCP protocol.
- Engine env vars are `TRANSLATOR_*` (`TRANSLATOR_MIC_DEVICE`, `TRANSLATOR_SPEAKER_DEVICE`, `TRANSLATOR_MEET_INPUT`, `TRANSLATOR_MEET_OUTPUT`, `TRANSLATOR_MY_LANG`, `TRANSLATOR_THEIR_LANG`) plus provider keys `DEEPGRAM_API_KEY`, `GROQ_API_KEY`.
- JSON event and command names are `snake_case` (`partial_transcript`, `tts_audio`, `set_config`); Rust serde uses `rename_all = "snake_case"`.

## Where to Add New Code

**New STT provider:**
- Implementation: `native/audio_engine/src/stt/<provider>_stt.rs`, then register a variant in `UnifiedSttSession` in `native/audio_engine/src/stt/mod.rs` and add the match arms for `send_audio`, `flush_pending`, `poll_transcript`, `close`.
- Wire into provider selection in `run_pipeline` (`native/audio_engine/src/engine.rs` around lines 640-660).
- Tests: a `src/bin/test_<provider>.rs` diagnostic, following the existing pattern.

**New translation provider:**
- Implementation: `native/audio_engine/src/translation/<provider>.rs`, add a branch in `TranslationEngine::translate` (`translation/mod.rs` line 121). Add the provider name to the `auto` fan-out.

**New TTS backend:**
- Implementation: `native/audio_engine/src/tts/<backend>.rs`, wire through `TtsEngine` in `tts/mod.rs`.

**New engine command or event:**
- Rust: add variant to `Command` or `Event` in `native/audio_engine/src/protocol.rs`, handle in `Engine::handle_command` (`engine.rs` line 170) or emit from `run_pipeline`.
- Elixir: add a `dispatch_event/2` clause in `lib/translator/audio_engine.ex`. For TCP access, add a `handle_command/1` clause in `lib/translator/command_server.ex`.
- Python: route in `web/routes.py`, client call through `send_engine_command` in `web/helpers.py`.

**New pipeline stage or per-direction behavior:**
- Shared logic goes in `native/audio_engine/src/engine.rs` (`run_pipeline`) so both directions get it. Direction-specific settings are passed as arguments from `start_pipelines` (around line 335).

**New HTTP route or UI feature:**
- Route: `web/routes.py` inside `register_routes`.
- Helpers: `web/helpers.py` (import `web/settings.py` constants only).
- Template: `web/templates/index.html` or `history.html`. JS: `web/static/js/app.js`. CSS: `web/static/css/style.css`.

**New DB table or call-history field:**
- `web/db.py`.

**New Elixir supervised process:**
- Add to the children list in `lib/translator/application.ex`. Use `PipelineSupervisor` for per-direction children.

**Utilities:**
- Python shared helpers: `web/helpers.py` (not `webhelpers.py`, which is a stale copy).
- Rust shared helpers: `native/audio_engine/src/tracelog.rs` for trace output, `audio/mod.rs` for device helpers.

## Special Directories

**`native/audio_engine/target/`:**
- Purpose: Cargo build output.
- Generated: Yes. Committed: No.

**`_build/`, `deps/`:**
- Purpose: Mix build and dependencies.
- Generated: Yes. Committed: No.

**`models/`:**
- Purpose: Piper voice models (`.onnx`, `.onnx.json`), downloaded at runtime through `/api/download-voice`.
- Generated: Yes (downloaded). Committed: Not covered by `.gitignore` as checked; treat as local data.

**`native/audio_engine/vendor/`:**
- Purpose: Placeholder for vendored Rust deps.
- Generated: No. Committed: Only `.gitkeep`.

**`.planning/`:**
- Purpose: Analysis output directory for mapping commands (this document lives in `.planning/codebase/`).
- Generated: Yes. Committed: Decided by the orchestrator.

**`.claude/`:**
- Purpose: Claude Code local state. Untracked (`git status` shows `?? .claude/`).

## Notes on Layout Anomalies

- `call-translator-win-git/` is a nested directory at the repo root. `git ls-files` lists nothing under it, and no reviewed code references it.
- `webhelpers.py` (184 lines, 7 functions) is an older copy of `web/helpers.py` (236 lines, 10 functions). Edit only `web/helpers.py`.
- `whisper_run.py`, `whisper_stt.py`, and `whisper_worker.py` at root overlap in purpose. Only `whisper_worker.py` is spawned at runtime (`native/audio_engine/src/stt/whisper_stt.rs`).
- `docs/superpowers/specs/` contains a dated design doc outside the GRACE docs set.

---

*Structure analysis: 2026-10-16*
