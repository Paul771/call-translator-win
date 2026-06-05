# Dual-Pipeline Real-Time Translation — Hang Fix & Full Duplex

Date: 2025-06-05
Status: Approved

## Problem

1. After ~5 minutes of continuous operation, the translation pipeline hangs. The Flask web UI shows "..." (SSE spinner) — no events arrive from the Rust engine.
2. The incoming pipeline (remote participant speech → English STT → translation to Russian → TTS to user's headphones) is fully disabled.
3. The remote participant hears only the original Russian speech (no translated audio forwarded).

## Root Cause of Hang

Most probable cascade:
1. Deepgram WebSocket `send()` is called with `set_blocking` mode and no timeout. If the TCP send buffer fills (network congestion, server backpressure), the call blocks indefinitely.
2. The main pipeline loop stops — no audio chunks drained from capture buffer, no events sent to Elixir.
3. Web UI SSE connection times out → shows "...".

Secondary cause: Elixir pipe buffer fills → Rust writer thread blocks on `BufWriter::flush()` → event channel fills (`bounded<Event>(256)`) → all `event_tx.send()` in `process_utterance()` block → processor thread blocks → `proc_tx.try_send()` fails → transcripts silently dropped.

## Approach

**Approach A (Minimal Viable Dual Pipeline)** — selected by user.

## Architecture

### Audio Routing

```
┌─────────────────────────────────────────────────────────┐
│                     Windows Audio                        │
│                                                          │
│  ┌──────────────┐    ┌──────────────┐                    │
│  │  Jabra Mic   │    │  Jabra HP    │                    │
│  │  (user's RU) │    │  (user hears)│                    │
│  └──────┬───────┘    └──────▲───────┘                    │
│         │                   │                            │
│         │    ┌──────────────┴────────┐                   │
│         │    │  CABLE Input          │                   │
│         │    │  (browser mic)        │                   │
│         │    └──────────▲────────────┘                   │
│         │               │                                │
│         │    ┌──────────┴────────────┐                   │
│         │    │  CABLE Output         │                   │
│         │    │  (browser speaker)    │                   │
│         │    └──────────┬────────────┘                   │
│         │               │                                │
│  ┌──────┴───────┐    ┌─▼──────────────┐                 │
│  │ OUTGOING     │    │  INCOMING      │                 │
│  │ pipe         │    │  pipe          │                 │
│  │              │    │                │                 │
│  │ Mic→STT(ru)  │    │ Cable Out→     │                 │
│  │ →Groq(ru→en) │    │ STT(en)→       │                 │
│  │ →TTS(en)     │    │ Groq(en→ru)    │                 │
│  │ →CABLE Input │    │ →TTS(ru)       │                 │
│  │              │    │ →Jabra HP      │                 │
│  └──────────────┘    └────────────────┘                 │
└─────────────────────────────────────────────────────────┘
```

### Data Flow

**Outgoing:**
```
cpal capture(Jabra Mic, 48kHz, F32, 1ch)
  → crossbeam channel (512 AudioChunks)
    → resample to 16kHz S16LE
      → Deepgram WS (non-blocking send, buffered on WouldBlock)
        → poll_transcript() returns final utterance
          → try_send to processor channel (capacity 16)
            → Processor thread:
              1. try_send Event::Transcript to event channel
              2. Groq translate: ru→en (HTTP, 30s timeout)
              3. try_send Event::Translation
              4. Piper TTS (en model)
              5. Play audio to CABLE Input device
              6. try_send Event::Metrics
```

**Incoming:**
```
cpal capture(CABLE Output, 16kHz, F32, 1ch)
  → same pipeline structure
    → Deepgram WS (en)
      → Groq translate: en→ru
        → Piper TTS (ru model)
          → Play audio to Jabra speakers
```

### Event Channel Protection

All `event_tx.send()` in `process_utterance()` changed to `try_send()`. If the channel is full, events are dropped with a warning. This prevents cascading backpressure from blocking the processor thread.

### Deepgram Non-blocking Send

- Socket stays in non-blocking mode always.
- `send_audio()` tries `ws.send()`. On `WouldBlock`, data is appended to `pending_audio: Vec<u8>`.
- Each pipeline loop iteration calls `flush_pending()` before sending new audio.
- `pending_audio` is bounded: if it exceeds 64KB, oldest data is discarded.

### Reconnect Circuit Breaker

- Max 5 reconnect attempts.
- Backoff: 2s, 4s, 8s, 16s, 30s (fixed thereafter).
- After all 5 attempts fail, send `Event::Error` to Elixir and stop the pipeline.

### Echo Suppression

- `echo_suppress: AtomicBool` shared between incoming and outgoing pipelines.
- Set `echo_suppress = true` while incoming TTS is playing.
- Outgoing pipeline checks `echo_suppress` before processing STT transcript.
- If suppressed, transcript is logged and dropped.
- Prevents feedback loop: TTS(ru) from Jabra speakers → picked up by Jabra mic → re-translated.

## Configuration

In `settings.json`:
- `mic_device`: `"Микрофон (Jabra EVOLVE 20)"`
- `speaker_device`: `"Динамики (Jabra EVOLVE 20)"`
- `meet_input_device`: `"CABLE Output (VB-Audio Virtual Cable)"`
- `meet_output_device`: `"Динамики (Jabra EVOLVE 20)"` — was null, set to Jabra speakers

Pipeline configuration in `start_pipelines()`:
- outgoing: mic=Jabra, output=CABLE Input, lang=ru, tts=en
- incoming: mic=CABLE Output, output=Jabra, lang=en, tts=ru

## Changes

### `native/audio_engine/src/stt/mod.rs`
- Remove `set_blocking()`/`set_nonblocking()` in `send_audio()`.
- Add `pending_audio` buffer field to `DeepgramStt`.
- `send_audio()`: non-blocking send, buffer on WouldBlock.
- Add `flush_pending()` method.
- Add `reset_pending()` for reconnect.

### `native/audio_engine/src/engine.rs`
- All `event_tx.send()` → `try_send()` with warn logging.
- Add `echo_suppress` to `run_pipeline()` signature.
- Outgoing: check `echo_suppress` before processing transcript.
- Incoming: set `echo_suppress` while TTS is playing.
- Uncomment and adapt incoming pipeline in `start_pipelines()`.
- Add reconnect backoff in main pipeline loop.
- Send `Event::Error` on permanent reconnect failure.
- Change outgoing TTS output device to CABLE Input.

### `native/audio_engine/src/main.rs`
- Writer thread: `recv_timeout(100ms)` instead of blocking `recv()`.
- Handle timeout by checking stop flag.

### `lib/translator/audio_engine.ex`
- Add configuration for incoming pipeline enablement.
- Pass CABLE Input device name for outgoing playback.

## Files Affected

| File | Status |
|------|--------|
| `native/audio_engine/src/stt/mod.rs` | Modify |
| `native/audio_engine/src/engine.rs` | Modify |
| `native/audio_engine/src/main.rs` | Modify |
| `settings.json` | Modify |
| `lib/translator/audio_engine.ex` | Modify |
| `docs/superpowers/specs/2025-06-05-dual-pipeline-hang-fix-design.md` | Create |

## Testing

### Unit Tests (Rust)
- `cargo build` — must compile without errors or warnings.
- `cargo run --bin test_onnx_session` — ONNX session init must succeed.
- `cargo run --bin test_tts` — TTS model loading must succeed.

### Integration Test
- Run the engine with `test_engine.ps1` — verify STT connects, TTS plays, metrics printed.
- Verify 10+ minute uptime without hang.

### Manual Test
- Join Telemost meeting.
- Set browser mic to CABLE Input, speaker to CABLE Output.
- Verify remote participant hears English translation.
- Verify user hears Russian translation of remote speech.
- Verify no echo/feedback loops.
- Verify hang-free operation for 30+ minutes.
