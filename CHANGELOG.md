# Changelog

## [Unreleased]

### Changed

- Groq translation model is configurable with `GROQ_MODEL`; the default is `openai/gpt-oss-20b`
  (`llama-3.3-70b-versatile` returned `model_not_found` on the test account). A transient Groq 404 is retried.
- Groq system prompt rewritten as an interpreter prompt (`translation/groq_prompt.txt`). On the
  prompt eval it matches Yandex in RU→EN and beats it in EN→RU (4.63 vs 4.47). End-to-end on 10
  phrases: 16/20 passed vs 12/20 for Yandex, median translate latency 350 ms vs 556 ms.

### Fixed

- Reply cleaning no longer cuts real translations (words such as "okay" or "first," and text inside
  quotation marks were dropped); it only removes think blocks, labels and one wrapping quote pair.
- One-word speech fragments are merged with the next segment instead of being voiced alone.
- Deepgram `utterance_end_ms` is clamped to at least 1000 (Deepgram rejects smaller values with HTTP 400).
- Echo suppression gate and the Deepgram drop counter in `engine.rs`.
- Yandex translation no longer panics on Cyrillic text (char-safe preview).

### Added

- End-to-end test suite in `tests/e2e/`: recorded or synthetic phrases go through the real engine in
  both directions and are scored (word recall, WER, chrF, LLM judge, round-trip).
- Prompt evaluation script `tests/e2e/prompt_eval.py`.

## [0.1.0] - 2026-04-05

### Initial open-source release

- Real-time bidirectional voice translation for video calls
- Deepgram Nova-3 STT with streaming WebSocket
- Groq LLM translation (llama-3.3-70b)
- Piper TTS with local ONNX inference (29 languages)
- Chrome tab audio capture (no BlackHole required for incoming audio)
- Web UI: live transcript, settings panel, call history, voice downloads
- Elixir OTP orchestration + Rust audio engine + Flask web server
- One-command setup via `setup.sh`
