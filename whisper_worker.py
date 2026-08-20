"""Persistent Whisper worker — loads model once, processes PCM chunks via stdin/stdout.

Protocol: 4-byte big-endian length prefix + payload, same as the Rust↔Elixir protocol.
stdin:  4-byte len + JSON {"cmd":"init","model":"tiny"} or {"cmd":"transcribe","audio_b64":"..."}
stdout: 4-byte len + JSON {"status":"ok"} or {"text":"...","language":"en","duration":0}
"""
import json, sys, struct, base64
from faster_whisper import WhisperModel

MODEL = None

def read_msg():
    raw = sys.stdin.buffer.read(4)
    if len(raw) < 4:
        return None
    length = struct.unpack(">I", raw)[0]
    data = sys.stdin.buffer.read(length)
    return json.loads(data) if data else None

def write_msg(obj):
    payload = json.dumps(obj, ensure_ascii=False).encode("utf-8")
    sys.stdout.buffer.write(struct.pack(">I", len(payload)))
    sys.stdout.buffer.write(payload)
    sys.stdout.buffer.flush()

def transcribe_audio_b64(b64_str):
    import tempfile, os
    pcm = base64.b64decode(b64_str)
    # Write PCM to a temp WAV in-memory via file (faster-whisper needs a file path)
    wav_path = os.path.join(tempfile.gettempdir(), f"whisper_worker_{os.getpid()}.wav")
    try:
        # Minimal WAV header: 16-bit mono PCM
        num_samples = len(pcm) // 2
        header = struct.pack(
            "<4sI4s4sIHHIIHH4sI",
            b"RIFF", 36 + len(pcm),
            b"WAVE", b"fmt ", 16, 1, 1, 16000, 16000 * 2, 2, 16,
            b"data", len(pcm)
        )
        with open(wav_path, "wb") as f:
            f.write(header)
            f.write(pcm)
        segs, info = MODEL.transcribe(
            wav_path, beam_size=1, language=None, vad_filter=True,
            vad_parameters=dict(min_silence_duration_ms=500, speech_pad_ms=400),
            no_speech_threshold=0.5, log_prob_threshold=-1.5,
            condition_on_previous_text=False
        )
        text = " ".join(s.text.strip() for s in segs if len(s.text.strip()) >= 2)
        return {"text": text, "language": info.language, "duration": 0}
    finally:
        try:
            os.remove(wav_path)
        except OSError:
            pass

while True:
    msg = read_msg()
    if msg is None:
        break
    cmd = msg.get("cmd")
    if cmd == "init":
        model_size = msg.get("model", "tiny")
        MODEL = WhisperModel(model_size, device="cpu", compute_type="int8")
        write_msg({"status": "ok"})
    elif cmd == "transcribe":
        if MODEL is None:
            write_msg({"error": "model not initialized"})
            continue
        audio_b64 = msg.get("audio_b64", "")
        if not audio_b64:
            write_msg({"text": "", "language": "??", "duration": 0})
            continue
        result = transcribe_audio_b64(audio_b64)
        write_msg(result)
    else:
        write_msg({"error": f"unknown command: {cmd}"})
