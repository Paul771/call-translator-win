# GRACE Framework - Project Engineering Protocol

## Keywords
realtime, call-translator, voice, stt, tts, translation, wasapi, deepgram, whisper, elixir, rust, python, flask, piper, wasapi, loopback, cpipe

## Annotation
Real-time bidirectional voice translator for video/voice calls. Translates both sides of a conversation live: speak your language, the other person hears theirs, and vice versa. Stack: Elixir (orchestrator) + Rust (audio engine) + Python (Flask web UI + Whisper STT).

## Core Principles

### 1. Never Write Code Without a Contract
Before generating or editing any module, create or update its MODULE_CONTRACT with PURPOSE, SCOPE, INPUTS, and OUTPUTS.

### 2. Semantic Markup Is Load-Bearing Structure
Markers like `// START_BLOCK_<NAME>` and `// END_BLOCK_<NAME>` are navigation anchors.

### 3. Knowledge Graph Is Always Current
`docs/knowledge-graph.xml` is the project map.

### 4. Verification Is a First-Class Artifact
Testing, traces, and log anchors are designed before large execution waves.

### 5. Top-Down Synthesis
Code generation follows: Requirements → Technology → Plan → Verification → Code + Tests

### 6. Governed Autonomy
Agents have freedom in HOW to implement, but not in WHAT to build.

## Grep-First Navigation

Navigation order:
1. Shared/public truth: `docs/knowledge-graph.xml`, `docs/development-plan.xml`, `docs/verification-plan.xml`
2. File-local/private truth: `MODULE_CONTRACT`, `MODULE_MAP`, `CHANGE_SUMMARY`
3. Full file reads only after the target module is narrowed

## File Structure
```
docs/
  requirements.xml
  technology.xml
  development-plan.xml
  verification-plan.xml
  knowledge-graph.xml
  operational-packets.xml
native/audio_engine/src/
  engine.rs          - Pipeline orchestrator
  audio/capture.rs   - WASAPI audio capture (cpal)
  audio/playback.rs  - Audio playback (cpal)
  audio/loopback.rs  - WASAPI loopback capture
  audio/mod.rs       - Device enumeration
  stt/mod.rs         - Deepgram STT + UnifiedSttSession
  stt/whisper_stt.rs - Local Whisper STT
  stt/yandex_stt.rs  - Yandex SpeechKit STT
  translation/mod.rs - Groq/Yandex/LiteLLM translation
  translation/yandex.rs - Yandex Translate API
  translation/litellm.rs - AI Portal LiteLLM translation
  tts/               - Piper TTS (ONNX)
lib/
  translator/audio_engine.ex - Elixir GenServer orchestrator
  translator/command_server.ex - TCP command server
web/
  routes.py          - Flask routes
  static/js/app.js   - Frontend JS
  templates/index.html - Settings UI
```

## Rules for Modifications

1. Read the MODULE_CONTRACT before editing any file.
2. After editing source files, update MODULE_MAP.
3. After adding/removing modules, update `docs/knowledge-graph.xml`.
4. After fixing bugs, add a CHANGE_SUMMARY entry.
