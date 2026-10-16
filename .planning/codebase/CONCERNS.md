# Codebase Concerns

**Analysis Date:** 2026-10-16

Scope: Rust audio engine (`native/audio_engine/src/`), Elixir orchestrator (`lib/translator/`), Python Flask UI and Whisper worker (`web/`, `whisper_worker.py`). Line numbers refer to the current worktree. Items marked **VERIFY** were inferred from code reading and were not reproduced at runtime.

Status of items in `docs/deferred-fixes-backlog.xml`: DF-3, DF-4, DF-5, DF-6, DF-7 are still present in code (confirmed below). Fixes 1-2 (reconnect chain, segmenter reset) are reflected in code.

---

## Priority Summary

| # | Severity | Area | Summary |
|---|----------|------|---------|
| 1 | High | Bidirectional | Echo suppression flags are written but never read; echo loop is unmitigated |
| 2 | High | Security | Plaintext API keys in `settings.json`, returned by unauthenticated `GET /api/settings` |
| 3 | High | Security | Unauthenticated localhost command channels (Flask :5050, TCP :5051) |
| 4 | High | Windows/Config | Hardcoded user-specific absolute paths in Elixir; settings file location mismatch with Python |
| 5 | High | Fallback chain | Deepgram drop counter resets on every successful reconnect, so STT escalation never fires |
| 6 | High | Latency | Translation and TTS run serially on the processor thread with up to 12 s blocking |
| 7 | Medium | STT | Whisper worker: no error handling, hardcoded 16 kHz header, temp-WAV roundtrip, 8 s blocking poll |
| 8 | Medium | Audio | Real-time capture callback does file I/O and logging; adaptive gain up to 100x |
| 9 | Medium | Audio | Naive linear resampler (no anti-alias filter) on the STT path |
| 10 | Medium | Stop/Lifecycle | Stop and reconnect cannot interrupt sleeps; `mem::forget` leaks loopback handle |
| 11 | Medium | Privacy | Transcripts and translations written to plaintext log files; API key prefixes logged |
| 12 | Medium | Build | `local-mt` feature wiring likely broken; onnxruntime binaries tracked in git |
| 13 | Medium | Testing | No CI; tests only in `vad/mod.rs`; no tests for segmenter, job queue, protocol, resampler |

---

## Security Considerations

**1. Plaintext API keys stored on disk and served to the browser**
- Risk: Deepgram, Groq, Yandex, and LiteLLM keys are persisted unencrypted to `<repo>/settings.json`.
- Files: `web/settings.py` (lines 7, 53-72, 87-89), `web/routes.py` (lines 48-52, 58-63).
- Current mitigation: `settings.json` is in `.gitignore`; `.env` is gitignored; `.env.example` is tracked with empty values only (no secrets in repo).
- Problem: `GET /api/settings` returns the full settings object, including keys, to any caller. `POST /api/settings` does `settings.update(data)` on arbitrary keys, so any caller can overwrite keys.
- Recommendations: Mask keys in GET responses (return `"***"` plus a `has_key` flag, as `_*_from_env` already does). Store keys in the Windows Credential Manager or DPAPI rather than in JSON. Allowlist the fields accepted by POST.

**2. Unauthenticated localhost control surfaces; DNS-rebinding exposure**
- Risk: Any local process can control the engine. Any web page that can rebind a DNS name to `127.0.0.1` can read the key-bearing settings.
- Files: `web.py` (line 29, `app.run(host="127.0.0.1", port=5050)`), `web/routes.py` (lines 424-429, `/cmd` forwards arbitrary `cmd` strings), `lib/translator/command_server.ex` (line 23 listens on :5051 with no auth; lines 62-76 accept loop).
- Current mitigation: Bound to loopback only.
- Recommendations: Validate the `Host` header on Flask (reject anything but `127.0.0.1:5050`/`localhost:5050`). Require a per-session token shared between the Python UI and the Elixir command server. Allowlist commands on `/cmd` instead of proxying any string.

**3. Command server kills whatever holds the port**
- Risk: On bind conflict, `kill_port_owner` runs `taskkill /F` on the PID of any process LISTENING on :5051, without verifying it is an orphaned engine.
- Files: `lib/translator/command_server.ex` (lines 33-50). Matching is `String.contains?(line, ":5051")`, which also matches `:50510`.
- Recommendations: Match the exact local address column and confirm the process image name before killing it.

**4. Verbose key-prefix logging**
- Risk: Partial key material ends up in stderr and `*.log` files.
- Files: `native/audio_engine/src/main.rs` (lines 53-57, first 8 characters of DEEPGRAM/GROQ keys), `native/audio_engine/src/engine.rs` (lines 67-71, first 4 characters of three keys), `native/audio_engine/src/translation/mod.rs` (lines 82-86, first 4 characters), `native/audio_engine/src/translation/yandex.rs` (line 14). The Elixir side logs only lengths (`lib/translator/audio_engine.ex` lines 260-261), which is the right pattern.
- Recommendations: Log only `present`/`len`, matching the Elixir side.

**5. Keys passed through environment**
- Files: `lib/translator/audio_engine.ex` (lines 276-281) injects keys as process env vars for the Rust child. Acceptable for a local child process, but any diagnostics that dump the environment will leak them.

---

## Bidirectional-Specific Risks

**6. Echo suppression is dead code (highest-impact audio issue)**
- Problem: `run_pipeline` receives `my_suppress` (`native/audio_engine/src/engine.rs` line 550, parameter name `my_suppress`), but no code ever loads it. The only reads of the suppression machinery are absent; only stores exist. Stores happen at line 858 (`_proc_echo.store(true)`) for the outgoing direction only, and at line 772 `_proc_echo` is bound to `set_other_suppress`. The flags `outgoing_suppress` and `incoming_suppress` (lines 355-356, 389-392, 424-426) therefore have no effect. The compiler should emit an unused-variable warning for `my_suppress`.
- Consequence: Incoming TTS plays to the user's speaker (`speaker_device`, engine.rs ~line 411-414, "Jabra speakers (user hears TTS)"). The outgoing pipeline captures the physical microphone, so it can transcribe the incoming translation, translate it again, and send it back to the call. The inverse path exists through the CABLE devices. Headphones are the only effective mitigation today.
- Related design flaw even if wired: suppression is a wall-clock timer (`dur_ms = audio_len/rate*1000 + 2000`, engine.rs lines 856-866), spawned per utterance. Overlapping utterances let an earlier timer clear the flag while later audio is still playing.
- Fix approach: Check `my_suppress` in the capture loop (drop chunks while set), or better, gate on actual playback completion from the `playback` module rather than estimated duration. Make the suppression flags per-direction and unit-testable. Consider a half-duplex mode (mute mic while own-side TTS plays) as a fallback when no headphones are detected.
- Also: `use_loopback` is `false` for both pipelines in the default path (engine.rs ~line 392 and ~line 430). The WASAPI loopback code path (`native/audio_engine/src/audio/loopback.rs`) is effectively unused by default.

**7. Stream ordering and shared global counter**
- `CAPTURE_CHUNK_COUNT` is a process-wide `static AtomicU64` (`native/audio_engine/src/audio/capture.rs` line 26) shared by both capture streams. Logging cadence (`prev == 0 || prev % 50 == 0`, line 71) therefore interleaves the two streams and misreports per-direction counts.
- Each pipeline has its own processor thread, so translations for the two directions can complete out of order. The UI keys events by `direction` (`lib/translator/audio_engine.ex` event handlers), so display is correct, but any consumer that assumes global ordering (call history in `web/db.py`) will see interleaved timelines.

**8. Mute flags are the only manual mitigation**
- `mute_outgoing` / `mute_incoming` (`native/audio_engine/src/engine.rs` lines 321-328) drop chunks in the capture loop (line ~1001). There is no UI path documented for this during a call, and muting does not stop TTS playback.

---

## Tech Debt

**9. Hardcoded absolute user path in the Elixir orchestrator (Windows-specific)**
- Issue: The project root, models directory, `settings.json` location, and `ORT_DYLIB_PATH` are fixed to `C:/Users/Pavel/AppData/Roaming/npm/call-translator-win`.
- Files: `lib/translator/audio_engine.ex` lines 250 (`models_base`), 277 (`ORT_DYLIB_PATH`), 326 (`read_settings`).
- Impact: Any other machine or checkout path breaks engine startup (models not found, ORT library not found). Worse, the Python UI writes `settings.json` to `BASE_DIR` (`web/settings.py` lines 6-7, the repo root), while Elixir reads from the AppData path. Keys saved in the web UI may never reach the engine unless the repo sits at the hardcoded path; the engine then silently falls back to env vars (`read_settings` logs a warning and uses an empty config, line ~345).
- Fix approach: Derive all paths from a single config value (`Application.get_env` / an env var set by `run.ps1`). Use one settings location shared by Python and Elixir.

**10. Hardcoded Python interpreter path**
- Files: `native/audio_engine/src/stt/whisper_stt.rs` line 32, `Command::new(r"C:\Program Files\Python312\pythonw.exe")`.
- Impact: Breaks on any other Python version or install location, and ignores the venv created by `setup.ps1`/`run.ps1`. Worker script is resolved from `current_dir()` (line 27-29), which depends on the launch directory.
- Also: the script path is passed with backslashes doubled (line 33, `.replace('\\', "\\\\")`). Rust's `Command` already quotes arguments on Windows, so the doubling is at best unnecessary and may produce a non-canonical path.
- Fix approach: Resolve the interpreter from the venv (`.venv\Scripts\pythonw.exe`) or an env var; pass the script path unmodified.

**11. `local-mt` feature wiring is inconsistent**
- Files: `native/audio_engine/Cargo.toml` line 37-38 declares `ct2rs` as an optional Windows-only dependency; the feature `local-mt = []` (line ~12) does not enable it. `native/audio_engine/src/translation/model.rs` line 9 and `translation/mod.rs` lines 3-4 and 66-70 gate code on `local-mt` and use `ct2rs` directly.
- **VERIFY**: `cargo build --features local-mt` on Windows is expected to fail with unresolved `ct2rs`. Fix: `local-mt = ["dep:ct2rs"]`.

**12. Tracked binary artifacts**
- `onnxruntime.dll` (14 MB) and `onnxruntime.dll.old` (11 MB) are tracked in git at the repo root. `.gitignore` lists `onnxruntime.dll.old`, but the file is tracked, so the ignore rule has no effect.
- Fix approach: `git rm --cached onnxruntime.dll onnxruntime.dll.old` and document the download step in `setup.ps1`. Keep history rewrite as a separate decision.

**13. Debug and diagnostic binaries compiled with the engine**
- `native/audio_engine/src/bin/` contains about 17 `test_*`/`check_*`/`list_*` binaries (e.g. `test_wasapi.rs`, `test_wavein.rs`, `test_onnx.rs`) with no `[[bin]]` gating. Cargo auto-discovers them, lengthening builds. `.gitignore` also excludes `test_*.py`/`check_*.ps1` patterns, which makes the tracked/untracked boundary unclear.
- Fix approach: Move diagnostics under a `dev-tools` feature or a separate crate.

**14. Mixed debug logging and user-visible logic**
- Logging goes to stderr, `pipeline_trace.log`, `deepgram_debug.log`, `groq_debug.log`, and `litellm_debug.log`, with overlapping helpers (`log_file` in `stt/mod.rs` line 16, `log_file_cap` in `audio/capture.rs` line 14, `log_file` in `audio/mod.rs` line 57, `log_to_file` in `translation/mod.rs`). Each opens and appends the file on every call.
- Fix approach: One tracing sink with a level filter.

---

## Known Bugs and Defects

**15. Deepgram drop counter resets on success; STT escalation never fires (DF-related, high)**
- Symptoms: Deepgram WebSocket drops about every 20 s (error 10054, per `docs/pipeline-latency-fix-plan.xml`). Each drop triggers a reconnect that succeeds, and success zeroes the counter.
- Files: `native/audio_engine/src/engine.rs` lines 874 (`MAX_DEEPGRAM_DROPS = 3`), 897-898 and 944 (escalation checks), 934 and 969 (`deepgram_drop_count = 0` on success).
- Impact: The auto-mode chain (Deepgram -> Yandex -> Whisper) only escalates after three *consecutive* failed reconnects. In the observed failure mode, the pipeline remains on the unstable provider indefinitely, producing repeated transcript gaps.
- Fix approach: Use a sliding-window drop count (e.g. drops in last 5 minutes) that is not zeroed on success, or escalate after N drops regardless of reconnect success.

**16. Stale-job check does not cancel work (DF-3, confirmed)**
- `job_queue.is_stale(job_id)` is evaluated after `process_utterance` returns (`native/audio_engine/src/engine.rs` lines 850-855). By that point translation and TTS playback have already run. The "stale job dropped" trace only logs.
- Files: `native/audio_engine/src/engine.rs` (lines 850-855), `native/audio_engine/src/pipeline/job_queue.rs` (whole file; `try_commit` at line 29, `is_stale` at line 40).
- Fix approach: Per backlog, either gate playback behind `try_commit` (requires `process_utterance` to return audio) or delete `JobQueue` and keep `TextSegmenter`.

**17. Whisper worker dies silently on any exception (DF-5, confirmed)**
- `whisper_worker.py` line 74 calls `transcribe_audio_b64` with no try/except. Any model or audio exception terminates the process. Rust's `stderr` is `Stdio::null()` (`native/audio_engine/src/stt/whisper_stt.rs`, `Command` setup lines 32-35), so the traceback is lost.
- Recovery is only via the 8 s read timeout (`whisper_stt.rs` line 123, `recv_timeout`), which then kills and respawns the worker.
- Fix approach: Wrap in try/except and always write `{"error": ...}`; in Rust, treat `error` as a fast failure and capture stderr to a file.

**18. Whisper worker hardcodes 16 kHz (DF-4, confirmed)**
- `whisper_worker.py` line 37 writes `16000` into the WAV header, and `byterate` `16000*2`, regardless of the `sample_rate` the Rust side actually sends (`whisper_stt.rs` line 97, `sample_rate` stored in the session).
- Fix approach: Send `sample_rate` in the `transcribe` message; write it into the header.

**19. Whisper poll blocks the pipeline thread for up to 8 s**
- The comment in `whisper_stt.rs` (around line 114-118) claims the pipeline loop is never blocked. In fact `poll_transcript` calls `rx.recv_timeout(8 s)` on the calling thread (line 124), so the capture/STT loop in `run_pipeline` is blocked for the whole round trip. Deepgram/Yandex audio is then not sent during that window.
- Fix approach: Make transcription asynchronous (send from a worker, poll results non-blocking), which is also the precondition for W3 partials.

**20. Translation error text is sent to the UI as translated text**
- `native/audio_engine/src/engine.rs` (around lines 1163-1200) emits `Event::Translation { text: format!("ERROR: {}", e) }` and similar `TIMEOUT after {}ms`. The UI renders these in the same slot as successful translations.
- Fix approach: Add an explicit `error` field or event variant.

**21. Final transcript can be silently dropped**
- `native/audio_engine/src/engine.rs` (~lines 1032-1045): when the processor channel is full, `try_send` failures are logged as `warn!` only. The UI receives no event. Same pattern for stable partials and translation/transcript events (`try_send` throughout `run_pipeline`).
- Impact: Under load (slow translation), finals are lost without any user-visible indicator, contrary to the "make failures visible" principle in `rules/quality-gates.md`.

**22. Segmenter fails on STT revisions**
- `native/audio_engine/src/pipeline/segmenter.rs` lines 17-27: if a new stable partial does not start with the previously translated text (any punctuation or casing revision), the whole text is re-translated and re-spoken, duplicating audio. The fallback branch (line 17-21) also resets `last_translated` to the full string, so repeated revisions compound.
- Fix approach: Compare on normalized tokens and only translate the word-level tail.

**23. Yandex STT auth header looks wrong (VERIFY)**
- `native/audio_engine/src/stt/yandex_stt.rs` line 102 sends `Authorization: Api-Key <iam_token>`, but the token was obtained from the IAM endpoint (line 45 `yandexPassportOauthToken`). IAM tokens are normally sent as `Authorization: Bearer <token>`. The IAM token is also fetched only once in `new()` (line 27) with no refresh, and IAM tokens expire (about 12 h), so long sessions will fail.
- **VERIFY** against Yandex SpeechKit docs before changing.

---

## Performance and Latency

**24. Serial translation and TTS on the processor thread**
- `process_utterance` (`native/audio_engine/src/engine.rs` lines 1120-1200) runs transcript emit -> translation (up to 12 s, `TRANSLATE_TIMEOUT_SECS` line 1149) -> TTS synthesis -> playback, all on one thread per direction. A slow translation delays every subsequent utterance in that direction.
- Plan reference: `docs/pipeline-latency-fix-plan.xml` F-3 and F-5 (W3 incremental TTS not yet implemented).

**25. Translation threads outlive their timeout**
- On `recv_timeout` expiry (engine.rs line 1161), the translation worker thread is not cancelled. HTTP clients have 60 s timeouts (`native/audio_engine/src/translation/mod.rs` line 94, `translation/litellm.rs` line 14), so orphaned threads and sockets accumulate during degraded network periods.
- Auto mode (`translation/mod.rs` lines 169-200) launches up to three provider calls per utterance; the losers continue running. This is DF-7 (quota x3, confirmed). W1-C2 (fastest-success) is already the auto-mode behavior; what remains is that explicit single-provider modes have no fallback at all.

**26. Naive linear resampler on the STT path**
- `native/audio_engine/src/engine.rs` lines 1310-1328: linear interpolation, no low-pass filter. Downsampling 48 kHz to 16 kHz aliases high-frequency content into the speech band, reducing STT accuracy, especially for sibilants.
- Also applied per chunk with no state across chunk boundaries, which can produce small discontinuities.
- Note: `native/audio_engine/src/tts/piper.rs` line 245-260 uses `rubato` sinc resampling for TTS output, so quality differs between paths.
- Fix approach: Use `rubato` (already a dependency) for the capture->STT path with carried state.

**27. Capture callback performs I/O and logging on the real-time thread**
- `native/audio_engine/src/audio/capture.rs` lines 67-75 run inside the cpal input callback: `info!` and `log_file_cap` (open + append on every logged chunk). Logging is conditional (`rms_raw > 0.001`), which is most of the time during speech.
- Impact: Risk of dropped capture buffers under load. Set `buffer_size: Fixed(320)` (line 56) is already tight.
- Fix approach: Push raw samples into a lock-free ring (the `ringbuf` dependency already exists) and do logging on another thread.

**28. Adaptive gain amplifies noise and bleed**
- `native/audio_engine/src/audio/capture.rs` lines 79-80: per-callback gain up to 100x for quiet input, and 50x when RMS is below 1e-5 (silence). Gain is recomputed per buffer with no smoothing, so it pumps and amplifies speaker bleed in the mic (relevant to concern 6).
- Coupling: the Whisper RMS gate at `whisper_stt.rs` line 90 (`rms < 0.25`) is tuned to this gain target. Changing one silently changes the other.

**29. Whisper transcription performance**
- `whisper_worker.py` line 43-48: `language=None` forces language detection per utterance; `beam_size=1`; model is `compute_type="int8"` on CPU (line 64). Each utterance is written to a temp WAV on disk (lines 29-42) and re-read by faster-whisper. This is the temp-WAV roundtrip that W2-C1 acceptance explicitly forbids (`docs/pipeline-latency-fix-plan.xml`); it is still present.
- Fix approach: Pass a numpy array directly to `WhisperModel.transcribe` (faster-whisper accepts float32 arrays), and pass a fixed language when the user's language is known.

**30. Client construction is already hoisted (W1-C3 partially done)**
- `Client::builder()` appears only in constructors: `translation/mod.rs` line 93 (shared), `translation/yandex.rs` line 16 (once per `YandexTranslator`, cached in `OnceLock`), `translation/litellm.rs` line 13, `stt/yandex_stt.rs` line 20 (per `YandexSttSession`, i.e. per reconnect). Reconnect-time client creation plus IAM fetch adds latency to every Yandex STT reconnect.

---

## Fragile Areas

**31. Reconnect and STT session lifecycle (`native/audio_engine/src/engine.rs` lines 871-1000)**
- Files: `native/audio_engine/src/engine.rs`, `native/audio_engine/src/stt/mod.rs` (Deepgram connect 15 s timeout at line 96-111; the connect thread is detached on timeout).
- Why fragile: Three provider types share one `UnifiedSttSession` enum with ad-hoc `match` arms that each repeat reconnect logic (lines ~897-985). Error paths use `continue` without clearing `need_reconnect` in at least the Whisper branch, so the loop can spin through backoff without progress.
- Safe modification: Extract a single `reconnect(provider_chain)` function and unit-test the escalation state machine (see concern 15).

**32. Stop path can hang**
- `stop_pipelines` (`native/audio_engine/src/engine.rs` lines ~448-455) joins pipeline threads. The reconnect sleep (`engine.rs` line 893, up to `MAX_BACKOFF_MS` = 30 s) and the translation/TTS waits are not interruptible by `stop_flag`. Stopping during a reconnect can take 30+ s.
- Fix approach: Sleep in small increments checking `stop_flag`, or use a condvar.

**33. Loopback handle leaked on purpose**
- `native/audio_engine/src/engine.rs` line 586: `std::mem::forget(loopback)`. The COM objects inside `LoopbackCapture` are never dropped; the stop flag stops the thread but resources are not released. Repeated start/stop cycles accumulate handles.
- Also: `CoInitializeEx` is called on the loopback thread (`audio/loopback.rs` line 69) and `CoUninitialize` is called in `audio/mod.rs` line 70 on a different path; the pairing is not guaranteed.

**34. Protocol framing has no size limit**
- `native/audio_engine/src/protocol.rs` line 114 (`u32::from_be_bytes` then `vec![0u8; len]`) and `whisper_stt.rs` `read_msg` (the same pattern) allocate up to 4 GB on a corrupted length prefix. A desynchronized stream can OOM the engine.
- Fix approach: Cap frame length (e.g. 16 MB) and fail the session on overflow.

**35. Unsafe FFI and manual COM**
- `native/audio_engine/src/stt/voxtral.rs` (lines 49-56, `unsafe impl Send/Sync` on `VoxtralEngine` with raw `*mut` handles; calls at 65-208). `native/audio_engine/src/audio/loopback.rs` (line 68 onward). Safety comments exist but the `Send`/`Sync` claim is unproven for a C library with a `stream` handle used from one thread at a time.

**36. Log retention deletes arbitrary `.log` files**
- `native/audio_engine/src/tracelog.rs` lines 40-55 (`prune_old_logs`) delete every `*.log` in the working directory older than 24 h, except `pipeline_trace.log`. The working directory is inherited from the Elixir BEAM process. Any user log file in the project root is removed.
- Fix approach: Prune only the known filenames, and only inside a dedicated `logs/` directory.

**37. `init_log` truncates the trace on every start**
- `native/audio_engine/src/tracelog.rs` lines 14-30 open `pipeline_trace.log` with `truncate(true)`. Diagnosing a failure after a restart is impossible unless the crash happened within the same run. Combined with concern 36, the only persistent record is the rotation-less `deepgram_debug.log`, which grows without bound (`log_file` helpers in `stt/mod.rs` line 16 and `audio/capture.rs` line 14 append without rotation).

**38. Elixir restart logic can schedule duplicate restarts**
- `lib/translator/audio_engine.ex` schedules `:restart_engine` from three places: `handle_cast(:restart_engine)` error path (line 195), `handle_info({port, {:exit_status, _}})` (line 218), and `handle_info(:restart_engine)` error path (line 233). No guard checks whether a restart is already pending or whether the old port is still open. `handle_cast(:restart_engine)` also blocks the GenServer with `Process.sleep(500)` and `Process.sleep(1000)` (lines 168, 177).
- **VERIFY**: whether pipelines are re-issued after a crash-restart; `handle_info(:restart_engine)` appears to reopen only the port.

**39. GenServer call timeouts**
- `lib/translator/audio_engine.ex` lines 28-60 use `GenServer.call` with default 5 s timeout. `start_pipelines` can exceed this during Piper/ONNX initialization (engine.rs line 691, `TTS_PRELOAD_TIMEOUT_SECS = 30`), causing caller exits.

**40. Whisper worker lifecycle is partly unmanaged**
- After a timeout (`whisper_stt.rs` lines 151-158) the worker is killed and `self.stdin`/`self.reader` are set to `None`. The next `poll_transcript` then errors with `Worker stdin closed` (line ~97-100), which the caller treats as a generic STT error and escalates via `need_reconnect`. Worker death is therefore handled by the drop counter, which resets (concern 15).

---

## Configuration and Windows-Specific

**41. Device names are English-only CABLE labels**
- `web/settings.py` lines 67-68 default `meet_input_device`/`meet_output_device` to `"CABLE Output (VB-Audio Virtual Cable)"` and `"CABLE Input (VB-Audio Virtual Cable)"`. Localized Windows or renamed devices fail device matching. Fix: match by substring on a stable ID, or enumerate and let the user pick.

**42. `.env` loading in PowerShell keeps quotes**
- `run.ps1` lines 12-19 parse `KEY=VALUE` with `Get-Content` and do not strip surrounding quotes, so `KEY="value"` sets the literal quotes in the environment. `run.sh` (line 8, `source .env`) behaves differently. Fix: strip matching quotes in `run.ps1`.

**43. Engine binary path and models path differ between tools**
- `config/config.exs` resolves `audio_engine.exe` relative to the repo, while `lib/translator/audio_engine.ex` line 250 uses the hardcoded AppData root for models. `native/audio_engine/src/tts/` and `Command::TtsPreview` (`engine.rs` ~line 233) use `TRANSLATOR_MODELS_DIR` defaulting to `./models` (relative to the engine's cwd). Three different roots for the same models directory.

**44. espeak-ng lookup is Unix-first**
- `native/audio_engine/src/tts/piper.rs` lines 245-256 check `espeak-ng` on PATH and `/opt/homebrew`, `/usr/local`, `/usr/bin` paths. No Windows install path (e.g. `C:\Program Files\eSpeak NG\`) is checked, so TTS init fails unless `espeak-ng` is on PATH.

---

## Testing Coverage Gaps

**45. No CI and minimal tests**
- No `.github/workflows` directory is tracked (only issue/PR templates under `.github/`). Rust unit tests exist only in `native/audio_engine/src/vad/mod.rs`. Elixir has no `test/` directory tracked. Python has no test files tracked (`test_*.py` is gitignored).
- Untested, high-risk logic: `native/audio_engine/src/pipeline/segmenter.rs` (`TextSegmenter::next_segment`), `native/audio_engine/src/pipeline/job_queue.rs`, `native/audio_engine/src/protocol.rs` framing, `engine.rs` `resample`, the reconnect escalation state machine (lines 871-1000), and `translation/mod.rs` auto-mode selection.
- Priority: High for segmenter and reconnect state machine (directly tied to concerns 15, 16, 22).

**46. No verification of pipeline metrics**
- `docs/verification-latency-matrix.xml` and `docs/pipeline-latency-fix-plan.xml` define `stt_ms`, `translate_ms`, `tts_ms`, `total_ms`, `provider_used` as metrics. The code emits `Event::Transcript.stt_ms` and `Event::Translation.translate_ms` (`engine.rs` lines 1130-1200), but `tts_ms`, `total_ms`, and `time_to_first_audio_ms` are not found in the emitting code. W1-C4 is only partially implemented.

---

## Dependencies at Risk

**47. Vendored native dependencies and unpinned runtime pieces**
- `ort = "2.0.0-rc.12"` (`native/audio_engine/Cargo.toml`) is a release candidate, loaded dynamically (`load-dynamic`) from a hardcoded DLL path (concern 9). The DLL version must match the crate's expected ORT version; mismatches fail at runtime, not at build.
- `native/audio_engine/vendor/` contains only `.gitkeep`; `ct2rs` (`local-mt`) is not vendored, so offline builds depend on crates.io.
- Python: `faster_whisper` is imported unconditionally in `whisper_worker.py` line 8 with no version pin in the worker itself (see `requirements.txt`).

---

## Missing Critical Features

**48. No operator-visible health for provider state**
- Provider failures (Deepgram drops, Yandex IAM failure, Groq quota) are emitted as `Event::Error` / `Event::Log` but not aggregated. The UI cannot show "running on fallback provider X" beyond the per-utterance `provider_used` field.

**49. No documented procedure for headphone-less operation**
- Given concern 6, speaker-to-mic feedback is the most likely user-facing failure in real calls, and no UI warning or setup check exists for it.

---

## Action Order (suggested)

1. Fix echo suppression wiring or add half-duplex gate (concern 6).
2. Fix Deepgram drop-counter escalation (concern 15) and the Whisper error path (concerns 17-18) from DF-5/DF-4.
3. Remove hardcoded user paths and unify settings location (concerns 9, 10, 43).
4. Mask keys in `GET /api/settings`, add Host validation and command allowlist (concerns 1-2).
5. Move capture logging off the real-time thread and make resampling filtered (concerns 26-27).
6. Stop trace/key-prefix leaks and fix `prune_old_logs` scope (concerns 4, 36-37).
7. Add tests for segmenter, reconnect state machine, protocol framing, and resampler (concern 45).

---

*Concerns audit: 2026-10-16*
