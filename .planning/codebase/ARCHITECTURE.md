# Architecture

**Analysis Date:** 2026-10-16

## Pattern Overview

**Overall:** Three-process pipeline. An Elixir/OTP supervisor owns a Rust audio engine child process (stdin/stdout protocol) and a Python Flask UI. Inside the Rust engine, each audio direction runs as an independent set of OS threads connected by bounded crossbeam channels.

**Key Characteristics:**
- Process boundaries are the main seams: Elixir <-> Rust over an Erlang Port with `{:packet, 4}` framing; Python <-> Elixir over a line-based TCP socket on `127.0.0.1:5051`; Python <-> browser over HTTP + SSE on `127.0.0.1:5050`.
- Two symmetric pipelines, `outgoing` (my voice to the other party) and `incoming` (their voice to me), run side by side with the same stage layout. Their only shared state is two echo-suppression flags.
- Rust side is thread-based, not async. Blocking WebSocket/HTTP calls live on dedicated threads, and stale work is dropped with a job ID counter.
- Event fan-out from Elixir goes through a log file (`test-log.txt`) that Flask tails for SSE, plus an in-memory queue for TTS audio. There is no direct push from Elixir to the browser.
- Elixir `Translator.Pipeline` processes hold per-direction history, but nothing in the repo reads them back to the UI.

## Processes and Layers

**Layer 1 - Python web UI (Flask, `web.py` + `web/` package):**
- Purpose: Settings UI, live transcript viewer, call history in SQLite, tab-capture translation endpoint.
- Location: `web.py`, `web/__init__.py`, `web/routes.py`, `web/helpers.py`, `web/db.py`, `web/settings.py`, `web/templates/`, `web/static/js/app.js`
- Contains: Flask routes, SSE stream, SQLite helpers, Groq/Yandex HTTP clients, Piper voice catalog downloader.
- Depends on: Elixir CommandServer (TCP 5051), `settings.json` and `test-log.txt` in the repo root, `calls.db`.
- Used by: The browser at `http://127.0.0.1:5050`.

**Layer 2 - Elixir orchestrator (OTP, `lib/translator/`):**
- Purpose: Supervises the Rust engine process, relays commands, dispatches engine events, runs the TCP command endpoint.
- Location: `lib/translator/application.ex`, `lib/translator/audio_engine.ex`, `lib/translator/command_server.ex`, `lib/translator/pipeline.ex`, `lib/translator/pipeline_supervisor.ex`, `lib/translator/config.ex`
- Contains: One `AudioEngine` GenServer that owns the Port; one `CommandServer` TCP listener; a `DynamicSupervisor` (`PipelineSupervisor`) that starts one `Pipeline` GenServer per direction.
- Depends on: The compiled Rust binary (path from `:audio_engine_path` app env), `settings.json`.
- Used by: Python via `CommandServer`.

**Layer 3 - Rust audio engine (`native/audio_engine/`):**
- Purpose: Device I/O (WASAPI capture/playback), VAD, streaming STT, translation, TTS synthesis, echo suppression.
- Location: `native/audio_engine/src/` (entry `main.rs`, orchestration `engine.rs`)
- Contains: Protocol codec, command dispatch, per-direction pipeline threads, STT/translation/TTS backends.
- Depends on: WASAPI via `cpal` and `wasapi`, ONNX Runtime (`ort`, load-dynamic, `onnxruntime.dll` at repo root), Deepgram/Yandex WebSockets, Groq/LiteLLM/Yandex Translate HTTP, optional Python Whisper worker subprocess.
- Used by: Elixir, via the Port.

## Process Topology

```
Browser (127.0.0.1:5050)
   |  HTTP / SSE (EventSource on /stream)
   v
Flask (web.py -> web/routes.py)  ---- reads test-log.txt (tail) ----+
   |  TCP line protocol 127.0.0.1:5051                              |
   v                                                                 |
Elixir CommandServer (command_server.ex)                             |
   |  GenServer.call                                                 |
   v                                                                 |
AudioEngine GenServer (audio_engine.ex) -- writes log lines --------+
   |  Erlang Port, {:packet, 4}, JSON both ways (stdin/stdout)
   v
audio_engine.exe (main.rs -> engine.rs)
   |-- outgoing pipeline threads
   |-- incoming pipeline threads
   |-- event-writer thread (stdout)
```

## Data Flow

**Outgoing direction (my speech -> their language, played into the call):**

1. Capture: `AudioCapture` (`native/audio_engine/src/audio/capture.rs`) reads the mic device `config.mic_device` (Jabra mic) and pushes `AudioChunk`s into `audio_tx` (bounded 512). No loopback (`use_loopback = false` in `engine.rs` `start_pipelines`).
2. Loop/STT: `run_pipeline` in `native/audio_engine/src/engine.rs` (around lines 536-1120) forwards chunks to a `UnifiedSttSession` (`native/audio_engine/src/stt/mod.rs`). Default provider is Deepgram (`stt/mod.rs` -> `stt/yandex_stt.rs` / `stt/whisper_stt.rs` on fallback). Language is `my_language`.
3. Partials: STT results arrive as `Partial`, `StablePartial`, or `Final` (`SttResultKind`). Partials and stable partials are emitted to Elixir as `partial_transcript` / `stable_partial_transcript`.
4. Segmentation and job control: a `TextSegmenter` (`native/audio_engine/src/pipeline/segmenter.rs`) computes only the new delta of stable text. A `JobQueue` (`native/audio_engine/src/pipeline/job_queue.rs`) gives each job an ID so stale translation/TTS results are discarded. A `Final` resets the segmenter.
5. Processor thread (spawned inside `run_pipeline`): translates the delta via `TranslationEngine` (`native/audio_engine/src/translation/mod.rs`, direction `my_language -> their_language`), emits `translation` / `translation_revised`, then synthesizes speech.
6. TTS: `TtsEngine` (`native/audio_engine/src/tts/mod.rs`, Piper in `tts/piper.rs`) loads lazily on a preload thread with the `tts_en_config` / `tts_en_model` voice. Output is resampled to the playback device's native rate.
7. Playback: samples go to `AudioPlayback` (`native/audio_engine/src/audio/playback.rs`) through `playback_tx` (bounded 64). Target device is `meet_output_device` (CABLE Input, the browser/Meet mic), falling back to `speaker_device` if empty or `default`.
8. Echo guard: when outgoing TTS plays, it sets `incoming_suppress` so the incoming pipeline ignores the CABLE loop.
9. Monitor: a 16 kHz PCM copy of each TTS result is sent as a `tts_audio` event (`engine.rs` around line 1257), which Elixir queues for `/api/poll-audio`.

**Incoming direction (their speech -> my language, played to my speakers):**

1. Capture: `meet_input_device` (CABLE Output, which carries the remote party's audio). No loopback.
2. STT: same `UnifiedSttSession` stack, language `their_language`.
3. Translation: `TranslationDirection::new(their_language, my_language)`.
4. TTS: `tts_ru_config` / `tts_ru_model` voice.
5. Playback: `speaker_device` (the user's headset/speakers).
6. Echo guard: sets `outgoing_suppress`, so the outgoing pipeline ignores the speaker bleed-through.

The two directions are built by the same `spawn_pipeline` / `run_pipeline` functions, selected by the `"outgoing"` / `"incoming"` match in `start_pipelines` (`engine.rs` around lines 335-460). Each direction gets its own `DeepgramStt` config, its own channels, and its own processor thread.

**Event path (Rust -> Elixir -> Python):**

1. Rust sends `Event` values (`native/audio_engine/src/protocol.rs`) over a bounded channel (256) to the `event-writer` thread in `main.rs`, which writes 4-byte big-endian length + JSON to stdout.
2. Elixir receives `{port, {:data, json}}` in `AudioEngine.handle_info/2` (`lib/translator/audio_engine.ex` around line 201) and dispatches via `dispatch_event/2`.
3. For transcripts, translations, and log lines, `dispatch_event` writes a text line to `test-log.txt` (`@log_file`, relative path) and calls `notify_pipeline/2` to forward to the matching `Translator.Pipeline` process.
4. For `tts_audio`, it appends to the per-process queue `Process.get(:audio_queue)` (capped at 5 items).
5. Flask `/stream` (`web/routes.py`) tails `LOG_FILE` (`BASE_DIR/test-log.txt`) and emits each new line as an SSE `data:` frame. `/api/poll-audio` drains the audio queue via TCP `poll_audio`.

**Command path (Browser -> Flask -> Elixir -> Rust):**

1. Browser `fetch('/cmd')` or `/api/engine/restart` (`web/static/js/app.js`) hits `web/routes.py`.
2. Flask calls `send_engine_command()` (`web/helpers.py`), which opens a TCP connection to `127.0.0.1:5051` and writes a line.
3. `CommandServer.handle_command/1` (`lib/translator/command_server.ex`) maps the line to `AudioEngine` calls. `start` calls `start_pipelines/0`; `preview:<lang>:<voice>` sends a `tts_preview` command; `restart` triggers `restart_engine_async`.
4. `AudioEngine` encodes the command with `Jason.encode!` and writes it with `Port.command`. Rust `read_command` (`protocol.rs`) decodes it and `Engine::handle_command` (`engine.rs` line 170) runs it.

**Startup and configuration path:**

1. `Translator.Application` (`lib/translator/application.ex`) starts children in `rest_for_one` order: `Config`, `AudioEngine`, `PipelineSupervisor`, `CommandServer`.
2. `AudioEngine.init/1` does not spawn the Rust process. The port opens on `start_pipelines` (`open_port/0`), which reads `settings.json`, builds the env list (device names, API keys, languages, voice paths), and spawns the binary with `{:spawn_executable, ...}`.
3. Rust reads its config from environment variables via `EngineConfig::from_env()` (`engine.rs` line 58). Runtime changes come through `SetConfig` commands (`apply_config`, `engine.rs` line 313).
4. On port exit, `AudioEngine` schedules `:restart_engine` after 2000 ms (`@restart_delay_ms`).

## Key Abstractions

**`UnifiedSttSession`:**
- Purpose: One streaming STT interface over several providers.
- Examples: `native/audio_engine/src/stt/mod.rs` (enum with `Deepgram`, `Yandex`, `Whisper` variants), `stt/yandex_stt.rs`, `stt/whisper_stt.rs`, `stt/voxtral.rs` (present but not wired into `UnifiedSttSession`).
- Pattern: Enum dispatch with `send_audio`, `flush_pending`, `poll_transcript`, `close`. Provider failover happens in `run_pipeline` on connection error (`engine.rs` around lines 640-660 and 296-370).

**`TranslationEngine`:**
- Purpose: Provider chain for text translation.
- Examples: `native/audio_engine/src/translation/mod.rs`, `translation/litellm.rs`, `translation/yandex.rs`, `translation/model.rs`.
- Pattern: Provider picked by `translation_provider` setting: `groq`, `litellm`, `yandex`, or `auto`. `auto` fans out to all configured providers in parallel and returns the first success (`translate`, `mod.rs` line 121).

**`TtsEngine` / `PiperTts`:**
- Purpose: Text to f32 PCM at the playback device rate.
- Examples: `native/audio_engine/src/tts/mod.rs`, `tts/piper.rs` (ONNX inference with espeak-ng phonemization).
- Pattern: Lazy background init on a separate thread to keep COM/WASAPI and ONNX Runtime initialization apart (see comments in `main.rs` and `engine.rs` lines 698-760).

**`JobQueue` and `TextSegmenter`:**
- Purpose: Drop stale translation/TTS work when new speech arrives; compute incremental deltas of stable text.
- Examples: `native/audio_engine/src/pipeline/job_queue.rs`, `native/audio_engine/src/pipeline/segmenter.rs`.
- Pattern: Monotonic job IDs; `try_commit` succeeds only for the newest ID.

**`Translator.Pipeline`:**
- Purpose: Per-direction in-memory history of transcripts, translations, and revised translations.
- Examples: `lib/translator/pipeline.ex`, supervised by `lib/translator/pipeline_supervisor.ex`.
- Pattern: `DynamicSupervisor` + `GenServer` with `handle_event/2` cast. Only `get_history/1` reads it back. No consumer exists in the repo, so this is currently a write-only buffer.

**Echo suppression flags:**
- Purpose: Stop each direction from re-capturing the other's TTS output.
- Examples: `outgoing_suppress` / `incoming_suppress` in `native/audio_engine/src/engine.rs` (`start_pipelines`, ~lines 345-350).
- Pattern: Shared `Arc<AtomicBool>` set by one pipeline's TTS and read by the other's capture loop.

**Mute flags and stop flag:**
- Purpose: Runtime mute per direction, global shutdown.
- Examples: `mute_outgoing`, `mute_incoming` (`Arc<AtomicBool>` set via `SetConfig`); `stop_flag` (`Arc<AtomicBool>`) checked by all loops; `stop_pipelines` (`engine.rs` line 443) joins the threads.

## Entry Points

**Python web app:**
- Location: `web.py` (`app.run(host="127.0.0.1", port=5050)`), `web/__init__.py` (creates Flask app and calls `register_routes`).
- Triggers: `run.ps1` / `run.sh`, user runs `python web.py`.
- Responsibilities: Serve UI, proxy engine commands, tail the log for SSE, persist calls to SQLite.

**Elixir application:**
- Location: `lib/translator/application.ex` (`Translator.Application`), `lib/translator.ex`.
- Triggers: `mix run` / OTP release startup.
- Responsibilities: Supervision tree, Port lifecycle, TCP command server on 5051.

**Rust engine binary:**
- Location: `native/audio_engine/src/main.rs` (`fn main`), `native/audio_engine/src/engine.rs` (`Engine::handle_command`).
- Triggers: Spawned by `AudioEngine.open_port/0` with `{:spawn_executable, engine_path}`.
- Responsibilities: Read commands on stdin, run pipelines, write events on stdout. Logs go to stderr so stdout stays a clean protocol channel.

**Auxiliary Rust binaries (diagnostics only, not used at runtime):**
- Location: `native/audio_engine/src/bin/*.rs` (for example `test_capture.rs`, `test_tts.rs`, `test_onnx_session.rs`).

**Whisper worker (optional STT backend):**
- Location: `whisper_worker.py` at repo root, spawned from `native/audio_engine/src/stt/whisper_stt.rs` (line 27-39) with a hardcoded `C:\Program Files\Python312\pythonw.exe`. Communicates over stdin/stdout with base64 PCM.
- Also present at repo root: `whisper_run.py`, `whisper_stt.py` (not referenced by the Rust engine or Flask app).

## Error Handling

**Strategy:** Errors are logged at each boundary and converted to events rather than thrown across processes. Failures inside a pipeline are reported as `Event::Error` or `Event::Log`, then the pipeline keeps running or restarts its STT session.

**Patterns:**
- Rust: `anyhow::Result` with `.context(...)`. Pipeline threads return errors that `stop_pipelines` logs on join (`engine.rs` line 443).
- STT reconnect: On Deepgram or Yandex failure, `run_pipeline` reopens a session of the same or fallback provider (`engine.rs` lines 296-370).
- Elixir: Port exit triggers a delayed restart (`lib/translator/audio_engine.ex` lines 216-219); unparsable engine output is logged and dropped (line 210).
- Python: `send_engine_command` returns a string error (`error:...`) instead of raising, so the UI stays up when the engine is down.

## Cross-Cutting Concerns

**Logging:**
- Rust: `log` + `env_logger` to stderr (`main.rs`), plus `tracelog::trace` writing per-pipeline trace lines (`native/audio_engine/src/tracelog.rs`, prunes logs older than 24 h).
- Elixir: `Logger`, plus `log_to_file/1` appending to `test-log.txt`.
- Python: `logging` configured in `web/__init__.py` (INFO) and `web.py` (DEBUG).

**Configuration:**
- Source of truth: `settings.json` at repo root (`web/settings.py` `SETTINGS_FILE`, read by Elixir `read_settings/0` and Flask `load_settings()`).
- Secrets and keys come from `settings.json` and the environment, not from code. `.env.example` shows the expected names.

**Validation:**
- Elixir `is_valid_device_name/1` filters device names from the engine (`lib/translator/audio_engine.ex` line 543).
- Python `/api/translate` and `/api/settings` do basic JSON shape checks in `web/routes.py`.

**Authentication:**
- None. The Flask app and TCP command server bind to localhost only.

## Known Architectural Gaps (current state)

- `CommandServer` `list_devices` returns hardcoded empty lists (`command_server.ex` line 129), so the UI gets devices from Python `web/helpers.py` instead.
- `Elixir AudioEngine.open_port/0` hardcodes `base_dir = "C:/Users/Pavel/AppData/Roaming/npm/call-translator-win"` for model paths (`lib/translator/audio_engine.ex` ~line 250), while Flask uses `BASE_DIR` (repo root) for `MODELS_DIR`. The two may disagree on where voices live.
- `test-log.txt` is written with a relative path by Elixir (`@log_file`) and read via an absolute path by Flask (`web/settings.py` `LOG_FILE`). They only match if the BEAM's working directory is the repo root.
- Two copies of the helper module exist: `webhelpers.py` (root) and `web/helpers.py`. Only `web/helpers.py` is imported by the package.
- `Translator.Pipeline` history is never read back into the UI.

---

*Architecture analysis: 2026-10-16*
