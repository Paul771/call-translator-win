# Coding Conventions

**Analysis Date:** 2026-10-16

Scope: `lib/` (Elixir), `native/audio_engine/src/` (Rust), `web.py`, `web/*.py`, `webhelpers.py`, `whisper_*.py` (Python), `web/static/js/*.js` (JavaScript). Conventions below are what the code actually does; where the code diverges from `AGENTS.md` or `CONTRIBUTING.md`, that is stated explicitly.

## Tooling Status (what is enforced vs. documented)

| Language | Formatter | Linter | Enforced? |
|----------|-----------|--------|-----------|
| Elixir | `mix format` via `.formatter.exs` (inputs: `lib/`, `config/`, `test/`) | none | No. `lib/translator/audio_engine.ex:246` has an unindented `defp open_port do`, so `mix format` has not been run on that file. |
| Rust | `cargo fmt` (documented in `CONTRIBUTING.md`) | `cargo clippy` (PR template checkbox in `.github/PULL_REQUEST_TEMPLATE.md`) | No. No `rustfmt.toml` or `clippy.toml`. 114 Rust lines exceed 100 columns (e.g. `native/audio_engine/src/stt/mod.rs` Deepgram URL and `tracelog::trace` calls). Two `#[allow(clippy::too_many_arguments)]` in `native/audio_engine/src/engine.rs:463,535`. |
| Python | none configured | none configured | No. No `pyproject.toml`, `setup.cfg`, `.flake8`, or `ruff.toml`. |
| JavaScript | none | none | No. No `package.json` in repo. |

No CI workflow exists (`.github/` contains only issue/PR templates). Nothing blocks a commit on formatting.

## Cross-Language Protocol Conventions

The three runtimes talk over a shared wire format. Keep these identical when adding messages:

- **Framing:** 4-byte big-endian length prefix + UTF-8 JSON payload. Rust side: `native/audio_engine/src/protocol.rs` (`read_command`, `write_event`). Elixir side: `{:packet, 4}` Port option in `lib/translator/audio_engine.ex`. Python side: `whisper_worker.py` (`read_msg`/`write_msg`, lines ~13-22).
- **Discriminator key:** `"cmd"` for Elixir to Rust commands, `"event"` for Rust to Elixir events. Values are `snake_case`.
  - Rust: `#[serde(tag = "cmd", rename_all = "snake_case")]` on `Command` and `#[serde(tag = "event", rename_all = "snake_case")]` on `Event` in `protocol.rs`.
  - Elixir: pattern-matched on string keys, e.g. `handle_cast({:event, %{"event" => "translation_revised", "text" => text} = event}, state)` in `lib/translator/pipeline.ex`.
- **Field names:** `snake_case` strings in JSON (`stt_ms`, `direction`, `pipelines`). Rust struct fields are also `snake_case`, so no rename is needed.
- **Errors on the wire:** Rust emits `{"event": "error", "message": ...}` (`Event::Error` in `native/audio_engine/src/protocol.rs:78`); Elixir logs it via `dispatch_event/2` at `Logger.error("Engine error: ...")` (`lib/translator/audio_engine.ex` ~line 488).
- **Stdout is reserved for protocol bytes.** `native/audio_engine/src/main.rs` routes `env_logger` to stderr. Never `println!` in engine code.

## Naming Patterns

**Elixir (`lib/`):**
- Modules: `Translator.<Area>` with file path mirroring the name: `Translator.AudioEngine` in `lib/translator/audio_engine.ex`, `Translator.CommandServer` in `lib/translator/command_server.ex`.
- Public functions: `snake_case`; predicates end in `?` (none found in current sources).
- Private helpers: `defp`, grouped under `# --- Private Helpers ---` (`audio_engine.ex`).
- Config keys: `snake_case` atoms (`:vad_threshold`, `:min_silence_ms`, `:transcript_timeout_ms`, `:sample_rate`), defined in `lib/translator/config.ex` `@defaults` and `config/config.exs`.
- Attribute constants: `@restart_delay_ms 2_000`, `@port 5051` (`command_server.ex`).

**Rust (`native/audio_engine/src/`):**
- Files and modules: `snake_case`. Directory modules use `mod.rs` (`stt/mod.rs`, `translation/mod.rs`, `vad/mod.rs`, `tts/mod.rs`, `audio/mod.rs`). Single-file modules sit beside a directory of the same name only when they are provider-specific (`stt/whisper_stt.rs`, `stt/yandex_stt.rs`, `translation/litellm.rs`, `tts/piper.rs`).
- Types: `UpperCamelCase`. Provider types suffix the role: `DeepgramStt`, `WhisperSttSession`, `TranslationEngine`, `TtsEngine`.
- Functions: `snake_case`. Provider-specific functions carry the provider as a suffix, e.g. `translate_yandex_raw` in `translation/mod.rs`.
- Constants: `SCREAMING_SNAKE_CASE`.
- Test helper bins: `native/audio_engine/src/bin/test_*.rs` and `check_*`/`list_*` diagnostics. These are not unit tests (see `TESTING.md`).

**Python (`web/`, root scripts):**
- Functions and variables: `snake_case` (`call_groq`, `send_engine_command`, `load_settings`, `_get_db`, `_init_db`).
- Module-private helpers: leading underscore (`_get_db`, `_ensure_call`, `_record_line`, `_call_lock`, `_list_audio_devices_windows` in `web/helpers.py`).
- Constants: `UPPER_SNAKE` in `web/settings.py` (`GROQ_MODEL`, `CMD_PORT`, `DB_FILE`, `DEFAULT_SETTINGS`).
- Flask routes: `/api/<noun>` (`/api/settings`, `/api/test-key`). Handler function name mirrors the path: `get_settings`/`post_settings` for `/api/settings`, `test_key` for `/api/test-key` (`web/routes.py`).

**JavaScript (`web/static/js/app.js`):**
- Functions: `camelCase` (51 `function xxxYyy` declarations).
- DOM references: `const` with an `El` suffix (`statusEl`, `toastEl`, `typingEl`) at the top of the file under `// ===== DOM refs =====`.
- Mutable application state: `let` under `// ===== State =====`. `var` is not used (0 uses; 185 `let`/`const`).
- Section banners use `// ===== Name =====`.
- Quotes: single quotes. Indent: 2 spaces. Semicolons are used on most statements.

**Environment variables and settings:**
- Env: `UPPER_SNAKE`, with the project prefix `TRANSLATOR_` for routing knobs (`TRANSLATOR_MIC_DEVICE`, `TRANSLATOR_MY_LANG`), and vendor keys unprefixed (`DEEPGRAM_API_KEY`, `GROQ_API_KEY`, `YANDEX_API_KEY`).
- Settings JSON keys: `snake_case` (`deepgram_api_key`, `translation_provider`, `my_language`), defined with defaults in `web/settings.py` `DEFAULT_SETTINGS`.

## Code Style

**Elixir:**
- `use GenServer` + `require Logger` at the top of each process module.
- Client API above server callbacks, separated by `# --- Client API ---` and `# --- Server Callbacks ---` (`lib/translator/pipeline.ex`, `lib/translator/audio_engine.ex`).
- `@impl true` on every callback. Public functions carry `@spec` (`audio_engine.ex` lines 24-63, `config.ex`, `pipeline.ex`). Use `@doc` on public functions in pipeline modules (`pipeline.ex`, `pipeline_supervisor.ex`), and `@moduledoc` on every module.
- Multi-clause `handle_cast`/`handle_call` dispatch on pattern-matched event maps rather than conditionals (`pipeline.ex` lines 45-101).
- State is a `defstruct` (`%Translator.Pipeline{}`, `%Translator.AudioEngine{}`), updated with `%{state | ...}`.
- Guard clauses on the state record guard against no-port cases: `handle_call({:set_config, _, _}, _, %{port: nil} = state)` returns `{:error, :port_not_open}`.

**Rust:**
- Module-level `//!` doc comments describe purpose (present in most files, e.g. `protocol.rs`, `tracelog.rs`, `engine.rs`, `segmenter.rs`).
- Item-level `///` doc comments on public API (`segmenter.rs`, `vad/mod.rs` 36 doc lines, `translation/model.rs` 21, `tts/piper.rs` 14).
- Large files use banner separators: `// ---------------------------------------------------------------------------` followed by a section name (`engine.rs:25-27, 132-134, 459-461, 531-533, 1115-1117, 1304-1306`), and in-function markers `// === TRANSLATION ===` (`engine.rs:1143, 1216, 1244`).
- Imports grouped: `std` first, then external crates, then `crate::`. `use log::{debug, error, info, warn};` is the standard logging import.
- Platform-specific code: `#[cfg(target_os = "windows")]` on dependencies in `Cargo.toml`, and `unsafe` FFI blocks for WASAPI/COM and `waveIn*` (`audio/mod.rs:69`, `audio/loopback.rs:68`, `bin/test_wavein.rs`). FFI declarations use `extern "C"` (`stt/voxtral.rs:28-29`).
- Feature flags in `Cargo.toml`: `directml`, `local-mt` (both empty by default).

**Python:**
- 4-space indentation. Lines are long: `web/routes.py` and `web/helpers.py` routinely exceed 100 columns; no limit is enforced.
- No type hints anywhere (0 annotated signatures across `web/` and root scripts). Do not assume typed APIs; match the existing untyped style unless you are introducing a typed module.
- Module docstring at the top of each file (`web.py`, `webhelpers.py`, `web/routes.py`, `web/db.py`, `whisper_stt.py`, `whisper_worker.py`). Function docstrings are rare.
- Import style: `web/` modules use explicit relative imports (`from .settings import (...)`, `from .helpers import (...)`). Root scripts use absolute `from web import app`.
- Section separators inside a module use `# ── Name ───` (`web/db.py`, e.g. `# ── DB access ──`).
- `# noqa: E402` is used for the one import that must follow `app` creation (`web/__init__.py`).
- Inline SQL is written as a single triple-quoted string passed to `executescript` (`web/db.py` `_init_db`).

**JavaScript:**
- Single-quoted strings, 2-space indentation, `const`/`let`, arrow functions for callbacks.
- Event listeners are attached in-line with `addEventListener` (no framework). Values from `input` elements are read through an accessor function (`input._getRealValue`) rather than a plain property (`app.js` lines ~20-30).

## Logging

**Elixir:** `Logger` with level-appropriate calls and string interpolation. Metadata keys `:direction` and `:pipeline` are declared in `config/config.exs` (`config :logger, :console, metadata: [:direction, :pipeline]`) and passed at call sites (`lib/translator/pipeline.ex`).
- Use `Logger.info` for lifecycle events, `Logger.debug` for per-event traces, `Logger.warning` for recoverable faults (JSON decode failure in `audio_engine.ex`), `Logger.error` for process-level failures.
- Engine status uses a prefix convention: `Logger.info("Engine: start_pipelines called")`, `Logger.info("Engine: sending to port: ...")`.
- Debug-time dumps of full JSON (`Logger.info("Engine: sending to port: #{json}")`) are present in `handle_call({:start_pipelines, ...})` and log every API key length on startup in `open_port/0`. Keep that in mind before adding more.

**Rust (three parallel mechanisms, in active use):**
1. `log` crate macros (`info!`, `warn!`, `error!`, `debug!`) initialised by `env_logger` writing to stderr (`native/audio_engine/src/main.rs:12-15`).
2. `tracelog::trace(component, STAGE, message)` in `native/audio_engine/src/tracelog.rs`. Writes `[HH:MM:SS.mmm] [pipeline] [STAGE] message` to stderr and `pipeline_trace.log` in the working directory. Stages: `CAPTURE | STT | TRANSLATE | TTS | PLAYBACK | EVENT | ERROR`, plus `INIT` and `DEVICE` in use. Used by `engine.rs`, `stt/mod.rs`, `audio/playback.rs`.
3. Ad-hoc bracket-tagged `eprintln!` with uppercase tags: `[ENGINE]`, `[TRANSLATION]`, `[AUTO]`, `[FALLBACK]`, `[LOCAL_MT]`, `[TRACE_LOG]`. Also `log_to_file("groq_debug.log", ...)` in `translation/mod.rs`.

Rule for new code: use `log::` macros for lifecycle messages and `tracelog::trace` for per-utterance pipeline stages. Do not add new `eprintln!` tags.

**Python:** the stdlib `logging` module with a shared logger named `"translator"` (`web/__init__.py`, `web/db.py`, `web/routes.py`, `web.py`). Use `logger = logging.getLogger("translator")` at module top. Whisper worker scripts write diagnostics with `print(..., file=sys.stderr)` because stdout is the protocol channel.

## Error Handling

**Elixir:**
- Return tuples: `{:ok, value}` / `{:error, reason}` from helpers (`open_port/0`, `read_settings/0`), with `case` to dispatch. Error reason is logged with `inspect(reason)`.
- Failure recovery: process exit or open failure schedules `Process.send_after(self(), :restart_engine, @restart_delay_ms)` and sets `status: :crashed` (`audio_engine.ex` `handle_info({port, {:exit_status, status}}, ...)`).
- Catch-all `rescue _ -> :ok` appears in `audio_engine.ex` (send/restart/`find_pipeline_pid`) and `command_server.ex` (`kill_port_owner/1`). These swallow errors silently. New code should log in the rescue clause (`Logger.warning`) instead of `_ -> :ok`.
- Tolerate bad input with a guarded `case Jason.decode(data)` and a logged warning, keeping the GenServer alive (`audio_engine.ex` `handle_info({port, {:data, data}}, ...)`).

**Rust:**
- Application errors use `anyhow::Result` and `anyhow::Context` (`engine.rs:12`, `audio/capture.rs:2`, `audio/mod.rs:5`). Attach context at each I/O boundary: `.context("Failed to spawn whisper_worker.py")?` (`stt/whisper_stt.rs:39`). Use `anyhow::bail!` for validation errors (`audio/capture.rs:147`, `audio/playback.rs:153`).
- The `Box<dyn std::error::Error>` pattern appears only in diagnostic bins under `src/bin/` (e.g. `test_wavein.rs`). Do not use it in library code.
- `unwrap()`/`expect()` in production code is rare: 4 occurrences outside `src/bin/` (`tracelog.rs` mutex lock; 3 in `translation/mod.rs`). Keep it that way; propagate with `?`.
- Errors that feed the pipeline are surfaced to Elixir as events and to the log, not panicked on. Provider fallback (`translation/mod.rs` ~lines 190-215) logs each failure to stderr and `groq_debug.log`, then continues to the next provider.

**Python:**
- Broad `except Exception as e:` is the norm (about 20 occurrences across `web/routes.py`, `web/helpers.py`, `webhelpers.py`). Results go back to the client as JSON: `return jsonify({"valid": False, "error": str(e)})` (`web/routes.py` lines ~72-111).
- HTTP status codes are used only for a few cases: `503` for `/health` loading state, `404` for not-found (`web/routes.py` lines 46, 360, 373). Validation failures in `/api/test-key` return HTTP 200 with an `error` field, so clients must check the body, not the status.
- `except Exception: pass`-style silent swallows exist in `webhelpers.py` and `web/helpers.py`. Prefer `logger.warning(..., exc_info=True)`.
- Socket commands to the engine return error strings prefixed `error:` (`web/helpers.py` `send_engine_command`) rather than raising.

**JavaScript:** fetch errors surface as toasts (`toastEl`); no custom error class.

## Comments

- Comments explain *why* (e.g. `# Don't start audio_engine here — it will be started on demand` in `audio_engine.ex` `init/1`; `// Drop the original sender so rx.iter() terminates once all threads finish.` in `translation/mod.rs`).
- Non-obvious constraints are commented at the site: the COM/WASAPI and stdout-reservation rules above.
- Debug leftovers exist and should not be copied: `# Debug: log Python path and pycaw availability` (`web.py`), and the "Debug version" module docstring plus per-chunk audio stats printed to stderr in `whisper_stt.py`.

## GRACE Markers (AGENTS.md) — Actual Usage

`AGENTS.md` requires, for every module: `MODULE_CONTRACT` (PURPOSE/SCOPE/INPUTS/OUTPUTS), `MODULE_MAP`, `START_BLOCK_<NAME>`/`END_BLOCK_<NAME>` anchors, and a `CHANGE_SUMMARY` entry after each bug fix.

**Observed usage: none.** A search of every tracked source file (`lib/`, `native/audio_engine/src/`, `web/`, root `*.py`, `web/static/js/`) returns 0 matches for `MODULE_CONTRACT`, `MODULE_MAP`, `START_BLOCK`, `END_BLOCK`, or `CHANGE_SUMMARY`. The markers appear only in `AGENTS.md` itself.

What *is* maintained in GRACE style:
- `docs/knowledge-graph.xml` (module nodes `M-ENGINE`, `M-STT`, ... with `<path>`, `<depends>`, `<verification-ref>`). All 7 referenced paths exist on disk.
- `docs/verification-plan.xml` (`V-M-*` verification nodes referenced from the knowledge graph).
- `docs/development-plan.xml`, `docs/requirements.xml`, `docs/technology.xml`, `docs/operational-packets.xml`.
- `docs/deferred-fixes-backlog.xml` and `docs/w{1,2,3}-implementation-backlog.xml` for change tracking, in place of `CHANGE_SUMMARY`.

Practical guidance for new code:
- Adding a module? Add an entry to `docs/knowledge-graph.xml`. Adding an in-file `MODULE_CONTRACT` is aspirational at present; no file follows that shape, so a new contract block would be the only one (choose a `// MODULE_CONTRACT` comment in Rust and `# MODULE_CONTRACT` in Python, and keep it short).
- `START_BLOCK`/`END_BLOCK` anchors do not exist in the code. Navigate by the banner comments above instead (`// ---- ... ----` in Rust, `# --- ... ---` in Elixir, `# ── ... ──` in Python).

## Function and Module Design

- **Size:** Large files are normal in this codebase: `native/audio_engine/src/engine.rs` (1328 lines), `lib/translator/audio_engine.ex` (577), `web/routes.py` (474), `web/helpers.py` (236), `web/static/js/app.js` (1064). New logic should go into the existing responsibility-scoped module (`stt/`, `translation/`, `tts/`, `audio/`, `vad/`, `pipeline/` in Rust; `web/helpers.py` for outbound HTTP/Groq calls in Python).
- **Parameters:** Positional, with keyword defaults for optional tuning (`call_groq(messages, api_key, temperature=0.1, max_tokens=None, timeout=10)`). Rust: `#[allow(clippy::too_many_arguments)]` is the accepted escape hatch for long pipeline-setup functions (`engine.rs:463,535`); prefer a config struct (`EngineConfig` in `engine.rs`) for new setup code.
- **Returns:** Elixir `{:reply, value, state}` / `{:noreply, state}` in GenServer callbacks. Rust `Result<T>` with `anyhow`. Python returns `jsonify(...)` from routes and plain values from helpers.
- **Config structs:** Rust `EngineConfig`, `VadConfig` (with `impl Default`) in `vad/mod.rs`. Use `..Default::default()` for partial overrides.

## Module Design

- **Elixir:** one public module per file. Supervision via `Translator.Application` -> `Translator.PipelineSupervisor` (DynamicSupervisor). Public API is a thin wrapper over `GenServer.call`/`cast`; the server callbacks are private.
- **Rust:** `pub mod` declarations in `mod.rs` re-export providers (`stt/mod.rs` declares `pub mod whisper_stt;`). `main.rs` declares all top-level modules with `mod x;`. Crate-internal references use `crate::` paths.
- **Python:** `web/__init__.py` builds the Flask app and calls `register_routes(app)`; route handlers are defined as closures inside `register_routes`, not at module level. Shared state (`_call_lock`, `_conn`) is module-level in `web/db.py`.
- **Barrel files:** `web/__init__.py` is the only Python package barrel. Rust `mod.rs` files serve as barrels.

## Known Convention Violations (do not copy)

- **Hardcoded user path** in `lib/translator/audio_engine.ex` `open_port/0`: `base_dir = "C:/Users/Pavel/AppData/Roaming/npm/call-translator-win"`. Other paths are derived from `Application.get_env` or `__DIR__`. Use configuration instead.
- **Duplicate module:** `webhelpers.py` (repo root) duplicates `web/helpers.py` (`call_groq`, `send_engine_command`, `get_voice_catalog`, `scan_voices`, `list_audio_devices`, and platform helpers). Nothing imports `webhelpers`. Only `web/helpers.py` is used by `web/routes.py` and `web.py`.
- **Duplicate import:** `web/routes.py` imports `get_groq_key` twice (lines 13-18 block and line 25 `from .settings import get_groq_key`).
- **Unused root scripts:** `whisper_stt.py` and `whisper_run.py` are not referenced by any code. The engine spawns `whisper_worker.py` (`native/audio_engine/src/stt/whisper_stt.rs:30`).
- **Name collision:** root `web.py` and the `web/` package share the import name `web`. Python resolves `web` to the package, so `web.py` only works as an entry point via `python web.py`.
- **Key exposure in logs:** `native/audio_engine/src/engine.rs:67-73` and `translation/mod.rs:81-85` print the first 8 characters of API keys plus length to stderr; `main.rs` does the same for `DEEPGRAM_API_KEY`/`GROQ_API_KEY`. Do not extend this pattern.
- **Silent rescue:** `rescue _ -> :ok` in `lib/translator/audio_engine.ex` and `lib/translator/command_server.ex`; `except Exception: pass` in `webhelpers.py` / `web/helpers.py`.
- **Unformatted Elixir:** `lib/translator/audio_engine.ex:246` `defp open_port do` at column 0.

---

*Convention analysis: 2026-10-16*
