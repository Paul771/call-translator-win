"""Audio helpers: normalise any input to 16 kHz mono PCM16 WAV, build WAVs from engine output."""

from __future__ import annotations

import base64
import shutil
import subprocess
import wave
from pathlib import Path

RATE = 16_000

_FFMPEG_FALLBACKS = [
    r"C:\Users\Pavel\AppData\Local\Microsoft\WinGet\Links\ffmpeg.exe",
]


def find_ffmpeg() -> str:
    found = shutil.which("ffmpeg")
    if found:
        return found
    for candidate in _FFMPEG_FALLBACKS:
        if Path(candidate).exists():
            return candidate
    raise RuntimeError("ffmpeg not found on PATH (needed to convert recordings to 16 kHz mono WAV)")


def normalize_to_wav16k(src: Path, dst: Path) -> float:
    """Convert any audio file to 16 kHz mono PCM16 WAV. Returns duration in seconds."""
    dst.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        find_ffmpeg(), "-y", "-loglevel", "error",
        "-i", str(src),
        "-ar", str(RATE), "-ac", "1", "-c:a", "pcm_s16le",
        str(dst),
    ]
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"ffmpeg failed for {src}: {result.stderr.strip()}")
    return wav_duration(dst)


def wav_duration(path: Path) -> float:
    with wave.open(str(path), "rb") as w:
        return w.getnframes() / float(w.getframerate())


def write_pcm16_wav(path: Path, pcm: bytes, rate: int = RATE) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(pcm)


def tts_events_to_wav(tts_events: list, path: Path, gap_ms: int = 250, pad_ms: int = 500) -> float:
    """Concatenate engine `tts_audio` events (base64 PCM16) into one WAV.

    Returns total duration in seconds, or 0.0 when there was no audio.
    """
    pcm = b""
    rate = RATE
    gap = b"\x00\x00" * int(rate * gap_ms / 1000)
    for ev in tts_events:
        rate = int(ev.data.get("sample_rate", RATE))
        chunk = base64.b64decode(ev.data.get("audio_b64", ""))
        if not chunk:
            continue
        if pcm:
            pcm += gap
        pcm += chunk
    if not pcm:
        return 0.0
    # Speech recognisers drop or garble words that start abruptly at sample 0; the
    # synthesized clip has no lead-in, so pad it the way a live line would be.
    pad = bytes(2) * int(rate * pad_ms / 1000)  # 16-bit silence
    duration = len(pcm) / 2 / rate
    write_pcm16_wav(path, pad + pcm + pad, rate)
    return duration
