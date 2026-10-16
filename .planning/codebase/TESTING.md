# Testing Patterns

**Analysis Date:** 2026-10-16

## Summary

The project has **no automated test suite that runs in CI**. Testing is:

1. A handful of Rust unit tests (4 tests, one of which asserts nothing).
2. Roughly 18 Rust diagnostic binaries in `native/audio_engine/src/bin/` that print to stdout/stderr and are run by hand.
3. Manual end-to-end validation with a real call (Google Meet, Zoom, etc.), as stated in `CONTRIBUTING.md` and `.github/PULL_REQUEST_TEMPLATE.md`.
4. Runtime log markers and trace files specified in `docs/verification-plan.xml`.

Elixir and Python have **no tests at all**.

## Test Framework by Language

| Language | Framework | Tests present | Config | Runs in CI |
|----------|-----------|---------------|--------|-----------|
| Rust (`native/audio_engine`) | built-in `cargo test` harness | 4 `#[test]` functions in `native/audio_engine/src/vad/mod.rs` | `native/audio_engine/Cargo.toml` (no `[dev-dependencies]`) | No |
| Elixir (`lib/`) | ExUnit (not wired up) | 0 | `mix.exs` has no `test_paths`, no test deps (only `{:jason, "1.4.4"}`) | No |
| Python (`web/`, root) | none (no pytest/unittest) | 0 | `requirements.txt` lists only `Flask==3.1.3`, `pycaw==20251023` | No |
| JavaScript (`web/static/js/`) | none | 0 | no `package.json` | No |

No `test/` directory exists in the repository. `.formatter.exs` names `test/**/*.{ex,exs}` as a formatter input, which is a leftover; the directory is absent.

**CI:** `.github/` contains only `ISSUE_TEMPLATE/bug_report.md`, `ISSUE_TEMPLATE/feature_request.md`, and `PULL_REQUEST_TEMPLATE.md`. There is no `.github/workflows/` directory.

## Run Commands

These are the commands that actually exist or are documented. Only the Rust commands target real test code; the others are diagnostic runs.

```bash
# Rust unit tests (the only #[test] functions in the repo)
cd native/audio_engine && cargo test

# Rust compile-only check (listed as module-check in docs/verification-plan.xml)
cd native/audio_engine && cargo check

# Rust lint (PR checklist, not enforced)
cd native/audio_engine && cargo clippy

# Diagnostic binaries (manual, not assertions)
cd native/audio_engine && cargo run --bin test_onnx_session   # ONNX session init
cd native/audio_engine && cargo run --bin test_tts            # TTS model load (declared in Cargo.toml)

# Elixir compile (verification-plan module-check; no tests)
mix compile
```

There is no watch mode and no coverage tooling configured. `cargo test` compiles the whole crate, including the Windows-only WASAPI/ONNX dependencies in `Cargo.toml`, so it needs the ONNX runtime library present (`onnxruntime.dll` at repo root, loaded via `ort` with `load-dynamic`) before tests that touch it can run. The four VAD tests below do not load the model.

I did not execute `cargo test` as part of this analysis; the pass/fail status of the four tests is therefore unverified.

## Rust Unit Tests

**Location:** inline `#[cfg(test)] mod tests` at the bottom of `native/audio_engine/src/vad/mod.rs` (lines 279-320). No `tests/` integration directory exists under `native/audio_engine/`.

**Structure:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_downsample_same_rate() {
        let input = vec![1.0, 2.0, 3.0];
        let output = downsample(&input, 16000, 16000);
        assert_eq!(input, output);
    }

    #[test]
    fn test_downsample_integer_ratio() {
        let input: Vec<f32> = (0..9).map(|i| i as f32).collect();
        // 48000 -> 16000 = factor 3, take every 3rd: 0, 3, 6
        let output = downsample(&input, 48000, 16000);
        assert_eq!(output, vec![0.0, 3.0, 6.0]);
    }
}
```

Conventions observed:
- Test names are `test_<unit>_<behaviour>` (`test_downsample_integer_ratio`).
- `use super::*;` inside the module to reach private helpers such as `downsample` (`vad/mod.rs:246`).
- Assertions use `assert_eq!` directly on `Vec<f32>` and scalar config fields. No property testing, no fixtures.

**Tests that check defaults:** `test_default_config` (`vad/mod.rs:298-305`) asserts `VadConfig::default()` has `threshold == 0.3`, `min_silence_ms == 500`, `speech_pad_ms == 30`, `sample_rate == 48000`. Note that the Elixir side configures `vad_threshold: 0.5` (`config/config.exs`, `lib/translator/config.ex` `@defaults`). The Rust test pins the Rust default only; it does not verify the value Elixir actually sends.

**Hollow test:** `test_state_machine_transitions` (`vad/mod.rs:307-312`) has a body consisting only of comments and no assertions. It always passes and provides no coverage. When adding state-machine coverage, either build a `VoiceActivityDetector` with a stub model or extract the transition function into a pure function and test that.

**Not covered by any test:** the `TextSegmenter` in `native/audio_engine/src/pipeline/segmenter.rs` (pure logic, easy to test: `next_segment` and `reset`), `pipeline/job_queue.rs`, the protocol framing in `native/audio_engine/src/protocol.rs` (`read_command`, `write_event`, pure byte handling), `translation/model.rs`, and the `stt/` and `tts/` providers.

## Rust Diagnostic Binaries (manual "tests")

`native/audio_engine/src/bin/` holds about 18 binaries. They are not `#[test]` functions. They print progress with `println!`/`eprintln!` and exit 0 regardless of hardware or model outcomes.

| Binary | Purpose |
|--------|---------|
| `test_onnx_session.rs` | ONNX session init; declared in `Cargo.toml` `[[bin]]` |
| `test_tts.rs` | TTS model load; declared in `Cargo.toml` `[[bin]]` |
| `test_capture.rs`, `test_capture2.rs`, `test_capture_fixed.rs`, `test_full.rs` | WASAPI capture and pipeline smoke runs |
| `test_wasapi.rs`, `test_wavein.rs`, `test_open_device.rs`, `test_devices.rs`, `test_enum.rs` | Device enumeration and open |
| `test_ws.rs` | Raw WebSocket connectivity probe over a list of URLs (sends 1 s of zero-valued PCM) |
| `test_ort_load.rs`, `test_onnx.rs` | ONNX runtime load |
| `test_env.rs`, `check_env2.rs`, `list_ascii.rs`, `list_devices_utf8.rs` | Environment and encoding diagnostics |

Cargo auto-discovers every file in `src/bin/`, so `cargo run --bin <name>` works for all of them. Only two are listed explicitly in `Cargo.toml`.

These binaries are the closest thing to integration tests. Treat them as reproduction tools: they are the place to add a probe when investigating a device or model problem, not a place for assertions.

Pattern used in them (`test_tts.rs`):
```rust
fn main() {
    println!("=== test_tts ===");
    // load model, print result
}
```

## Elixir Testing (not present)

ExUnit is available (Elixir `~> 1.19` in `mix.exs`) but nothing is set up:
- No `test/test_helper.exs`.
- `mix.exs` `project/0` has no `test_paths` or `aliases: [test: ...]`.
- Modules with pure logic that would be the first candidates:
  - `Translator.Pipeline` (`lib/translator/pipeline.ex`): `handle_cast({:event, ...})` clauses accumulate transcripts and translations; `handle_call(:get_history, ...)` reverses them. Testable with `start_supervised!` and `GenServer.cast`/`call`.
  - `Translator.Config` (`lib/translator/config.ex`): `load_config/0` merges `@defaults` with `Application.get_env`. Testable with `Application.put_env` + `Config.reset/0`.
  - `Translator.CommandServer` `handle_command/1` clauses (`command_server.ex` lines 82+) are pure dispatch.
- `Translator.AudioEngine` spawns the Rust binary through a Port and reads a hardcoded Windows path. It would need the port opener injected before it could be tested without the binary.

Run convention, if added: `mix test` (no tags, no async config yet). Place files at `test/translator/<module>_test.exs`, matching `lib/translator/<module>.ex`. Note that `.gitignore` ignores `test_*.exs`; name test files `<module>_test.exs` (suffix form) to avoid being silently excluded from git.

## Python Testing (not present)

No test runner is installed or referenced. `requirements.txt` has only `Flask==3.1.3` and `pycaw==20251023`.

Relevant units if tests are added (pytest is the natural choice; add it to `requirements.txt` or a dev requirements file):
- `web/settings.py`: `load_settings()` merges `DEFAULT_SETTINGS` with `settings.json`; `get_groq_key()` falls back to `GROQ_API_KEY`. Testable with `monkeypatch.setenv` and a temp `SETTINGS_FILE` (patch the module constant).
- `web/routes.py`: `register_routes(app)` attaches closures to a Flask app. Use `app.test_client()`. `/health` (line ~41) depends on a socket to `127.0.0.1:5051`; patch `socket.socket`.
- `web/db.py`: `_get_db()` opens `DB_FILE` (`calls.db` at repo root). Point `web.settings.DB_FILE` at a temp file before importing, because `web/db.py` runs `_init_db()` and closes orphan calls at import time.
- `web/helpers.py`: `call_groq(...)` uses `urllib.request.urlopen`; patch `urllib.request.urlopen`.
- `whisper_worker.py`: length-prefixed protocol functions `read_msg`/`write_msg` take stdin/stdout buffers and can be tested with `io.BytesIO`. Importing the module imports `faster_whisper` at top level, so stub it in `sys.modules` first.

**Git ignore warning:** `.gitignore` excludes `test_*.py`, `flask_test.py`, `check_*.py`, `debug_*.py`, and `test_*.ps1` under "Test files". A new file named `test_routes.py` would be silently excluded from commits. Use `tests/test_routes.py`-style names only after removing or narrowing those patterns, or name files without the `test_` prefix and keep them out of the ignored patterns.

## Mocking

- **Rust:** no mocking library in `Cargo.toml`. The VAD test explicitly avoids mocking by not constructing the detector (`vad/mod.rs:309-311`). Providers are concrete enums/structs (`UnifiedSttSession`, `TranslationEngine`), so a test seam would require extracting pure functions rather than trait-based mocks.
- **Python:** no mocks in the tree. When adding tests, use `unittest.mock.patch` (stdlib) on `urllib.request.urlopen`, `socket.socket`, and `web.settings` constants.
- **Elixir:** no `Mox` dependency. Behaviours are not defined for `AudioEngine` or `CommandServer`, so injection would have to be added (e.g. pass the port-open function through `start_link` opts).

**What to mock:** external HTTP (Groq, Yandex, Deepgram), the Rust engine process (Port), audio devices, and the filesystem paths in `web/settings.py`.

**What not to mock:** the length-prefixed framing code, `downsample`, `TextSegmenter::next_segment` and other pure transforms. These are cheap to test directly.

## Fixtures and Test Data

- No fixture directory exists. The Rust VAD test builds inputs inline (`vec![1.0, 2.0, 3.0]`, `(0..9).map(...)`).
- Audio test data is not checked in. The `onnxruntime.dll` and model files are runtime dependencies, not fixtures.
- `docs/superpowers/specs/2025-06-05-dual-pipeline-hang-fix-design.md` (Testing section) describes a `test_engine.ps1` that does not exist in the tree; it is ignored by `.gitignore` (`test_*.ps1`). Do not rely on it.

## Coverage

**Requirements:** none enforced. `docs/technology.xml` `<mocking-policy>` states: "No formal test suite. Integration testing via manual pipeline run with audio input."

**Coverage tooling:** none. Rust would need `cargo-llvm-cov` or `cargo-tarpaulin`; Elixir would need `mix test --cover` (after a test suite exists).

**Estimated coverage:** effectively zero for Elixir and Python; a small fraction of Rust (only `vad/mod.rs` helper functions).

## Verification Approach (docs/verification-plan.xml)

The project's real verification is runtime observation. Each module in `docs/verification-plan.xml` (`V-M-ENGINE`, `V-M-STT`, ...) defines:
- `<module-checks>`: `cargo check` and `mix compile`.
- `<scenarios>`: success/failure runs (e.g. "Pipeline produces [outgoing][METRICS] and [incoming][METRICS] within 60s"; failure "0x800706CC blocks AudioCapture creation").
- `<required-log-markers>`: exact log lines that must appear, such as `[engine] Pipeline running`.
- `<test-files>`: runtime artifacts to inspect, which are logs, not test code: `pipeline_trace.log`, `deepgram_debug.log`, `groq_debug.log`.

Log sources feeding these checks:
- `native/audio_engine/src/tracelog.rs` writes `pipeline_trace.log` (truncated at each start; files older than 24 h are pruned).
- `native/audio_engine/src/translation/mod.rs` writes `groq_debug.log`.
- `lib/translator/audio_engine.ex` writes `test-log.txt` (`@log_file`), and `web/settings.py` `LOG_FILE` points to the same name.

To verify a change: start the app (`run.ps1` / `run.sh`), place a real call, then grep the trace log for the markers listed in the verification plan. `docs/pipeline-latency-fix-plan.xml` and `docs/verification-latency-matrix.xml` define latency targets (e.g. "p95 translate_ms drops by at least 30 percent on degraded-network tests") that are checked from these logs.

## Manual Test Checklist (from PR template)

`.github/PULL_REQUEST_TEMPLATE.md`:
- [ ] Tested locally with a real call
- [ ] No API keys or secrets in the diff
- [ ] Rust: `cargo clippy` passes
- [ ] Python: no new warnings

Record in the PR: which app and browser were used, and whether the trace log shows the expected markers.

## Test Types

**Unit tests:** Rust only, 4 tests (`vad/mod.rs`), one empty.

**Integration tests:** none automated. Manual pipeline runs with real audio devices (`test_full.rs`, `test_capture*.rs` as probes).

**End-to-end tests:** none automated. Manual real-call testing is the E2E process.

**Smoke test:** `docs/pipeline-latency-fix-plan.xml` references a "smoke test" with no final transcript loss in both directions; this is a manual procedure.

## Common Patterns (for future tests)

**Rust async-free pure function test:**
```rust
#[test]
fn test_next_segment_resets_on_divergence() {
    let mut seg = TextSegmenter::new();
    assert_eq!(seg.next_segment("hello world"), Some("hello world".into()));
    assert_eq!(seg.next_segment("goodbye"), Some("goodbye".into()));
}
```
(Target: `native/audio_engine/src/pipeline/segmenter.rs`. Add `#[cfg(test)] mod tests` at the bottom of the file, following the `vad/mod.rs` layout.)

**Elixir GenServer event test (proposed, ExUnit):**
```elixir
test "accumulates final transcripts in order" do
  {:ok, pid} = Translator.Pipeline.start_link(:outgoing)
  Translator.Pipeline.handle_event(pid, %{"event" => "transcript", "text" => "a"})
  Translator.Pipeline.handle_event(pid, %{"event" => "transcript", "text" => "b"})
  assert %{transcripts: [%{text: "a"}, %{text: "b"}]} = Translator.Pipeline.get_history(pid)
end
```
(`handle_event/2` is a cast, so the call to `get_history/1` acts as a synchronization barrier. This works with the existing API in `lib/translator/pipeline.ex` without changes.)

**Error path (Python, proposed):**
```python
def test_test_key_reports_invalid(client, monkeypatch):
    # Patch urllib.request.urlopen to raise HTTPError(401); expect {"valid": False, ...}
    ...
```
Error responses in `web/routes.py` return HTTP 200 with `"valid": False`, so assert on the JSON body and not on `response.status_code`.

## Gaps Worth Prioritising

| Area | Risk | Suggested first test |
|------|------|----------------------|
| `native/audio_engine/src/protocol.rs` framing | A wire-format change breaks Elixir silently | round-trip `write_event` / `read_command` over `Cursor<Vec<u8>>` |
| `native/audio_engine/src/pipeline/segmenter.rs` | Delta logic drives what gets translated; no coverage | the `next_segment` divergence/reset cases above |
| `vad/mod.rs` `test_state_machine_transitions` | Empty test gives false confidence | extract transition logic into a pure function |
| `lib/translator/audio_engine.ex` `dispatch_event/2` | Event routing for all engine messages | start with the pure clauses; inject port |
| `web/settings.py` `load_settings` | Settings merge and env fallback | pytest with `monkeypatch` |
| Cross-language VAD default | Rust default 0.3 vs Elixir 0.5 | single source of truth, then one assertion on each side |

---

*Testing analysis: 2026-10-16*
