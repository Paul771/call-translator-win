"""Stream recorded phrases to Deepgram exactly like the engine does and compare STT models.

The engine streams 20 ms chunks in real time and keeps only the final results. This
probe does the same outside the engine and prints what each model returned, so a
transcript problem can be pinned on the recogniser instead of the pipeline.

    python tests/e2e/probe_stt.py --phrases tests/e2e/phrases.json --models nova-2-general nova-3 --repeat 5
"""

from __future__ import annotations

import argparse
import json
import sys
import threading
import time
import wave
from pathlib import Path

import websocket  # websocket-client

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from e2e import audio_utils, scoring  # noqa: E402
from e2e.providers import load_project  # noqa: E402
from e2e.runner import load_manifest  # noqa: E402

CHUNK_MS = 20
TAIL_MS = 2500


def read_pcm(path: Path) -> bytes:
    with wave.open(str(path), "rb") as w:
        assert w.getframerate() == 16000 and w.getnchannels() == 1 and w.getsampwidth() == 2
        return w.readframes(w.getnframes())


def stream_once(pcm: bytes, key: str, model: str, language: str, endpointing: int, utterance_end: int,
                stall_at: float = 0.0, stall_ms: int = 0) -> dict:
    """One streaming session. Sending and receiving share one thread: websocket-client
    does not support a reader thread and a writer thread on the same socket."""
    url = (
        "wss://api.deepgram.com/v1/listen"
        f"?model={model}&language={language}&encoding=linear16&sample_rate=16000&channels=1"
        f"&multichannel=false&interim_results=true&endpointing={endpointing}"
        f"&utterance_end_ms={utterance_end}&vad_events=true"
    )
    ws = websocket.create_connection(url, header=[f"Authorization: Token {key}"], timeout=15)
    ws.settimeout(0.02)
    finals, interims, t0 = [], [], time.monotonic()

    def drain() -> bool:
        """Read whatever is waiting. False once the server has closed the stream."""
        while True:
            try:
                opcode, data = ws.recv_data()
            except websocket.WebSocketTimeoutException:
                return True
            except (websocket.WebSocketException, OSError):
                return False
            if opcode == 8:
                return False
            if opcode != 1:
                continue
            msg = json.loads(data.decode("utf-8", "replace"))
            if msg.get("type") != "Results":
                continue
            text = msg["channel"]["alternatives"][0].get("transcript", "").strip()
            if not text:
                continue
            at = round(time.monotonic() - t0, 2)
            (finals if msg.get("is_final") else interims).append((at, text))

    step = 16000 * 2 * CHUNK_MS // 1000
    chunks = [pcm[i:i + step] for i in range(0, len(pcm), step)] + [bytes(step)] * (TAIL_MS // CHUNK_MS)
    start = time.monotonic()
    stalled = False
    alive = True
    for i, chunk in enumerate(chunks):
        if stall_ms and not stalled and i * CHUNK_MS / 1000 >= stall_at:
            time.sleep(stall_ms / 1000)
            start += stall_ms / 1000
            stalled = True
        wait = start + i * CHUNK_MS / 1000 - time.monotonic()
        if wait > 0:
            time.sleep(wait)
        try:
            ws.send_binary(chunk)
        except (websocket.WebSocketException, OSError):
            alive = False
            break
        alive = drain()
        if not alive:
            break
    if alive:
        try:
            ws.send(json.dumps({"type": "CloseStream"}))
        except (websocket.WebSocketException, OSError):
            pass
        deadline = time.monotonic() + 1.5
        while time.monotonic() < deadline and drain():
            pass
    try:
        ws.close()
    except (websocket.WebSocketException, OSError):
        pass
    return {"finals": finals, "interims": interims}


def main() -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--phrases", required=True)
    ap.add_argument("--only", nargs="*")
    ap.add_argument("--models", nargs="+", default=["nova-2-general", "nova-3"])
    ap.add_argument("--repeat", type=int, default=3)
    ap.add_argument("--endpointing", type=int, default=200)
    ap.add_argument("--utterance-end", type=int, default=1000)
    ap.add_argument("--stall-at", type=float, default=0.0, help="seconds into the audio where delivery pauses")
    ap.add_argument("--stall-ms", type=int, default=0, help="length of that delivery pause")
    ap.add_argument("--out", help="write all runs as JSON")
    args = ap.parse_args()

    cfg = load_project(HERE.parent.parent)
    key = cfg.secrets.deepgram
    if not key:
        print("no Deepgram key")
        return 2
    langs, phrases = load_manifest(Path(args.phrases).resolve())
    if args.only:
        phrases = [p for p in phrases if p.id in args.only]
    my = langs.get("my", "ru")
    their = langs.get("their", "en")

    work = HERE / "results" / "probe_fixtures"
    runs, summary = [], {}
    for p in phrases:
        lang = my if p.direction == "outgoing" else their
        wav = work / f"{p.id}.wav"
        audio_utils.normalize_to_wav16k(p.audio, wav)
        pcm = read_pcm(wav)
        for model in args.models:
            for rep in range(1, args.repeat + 1):
                try:
                    res = stream_once(pcm, key, model, lang, args.endpointing, args.utterance_end,
                                      args.stall_at, args.stall_ms)
                except (websocket.WebSocketException, OSError) as e:
                    print(f"{p.id} {model} #{rep}: connection failed ({type(e).__name__})")
                    continue
                joined = " ".join(t for _, t in res["finals"])
                recall = scoring.word_recall(p.text or "", joined)
                exact = scoring.normalize(joined) == scoring.normalize(p.text or "")
                runs.append({"id": p.id, "model": model, "rep": rep, "finals": res["finals"],
                             "joined": joined, "recall": round(recall, 3), "exact": exact})
                s = summary.setdefault((p.id, model), [0, 0, 0.0])
                s[0] += 1
                s[1] += int(exact)
                s[2] += recall
                print(f"{p.id} {model:15} #{rep}: {'OK ' if exact else 'DIFF'} finals={[t for _, t in res['finals']]}")

    print("\nsummary (exact match of the joined finals / mean word recall)")
    for (pid, model), (n, exact, rec) in summary.items():
        print(f"  {pid:6} {model:15} exact {exact}/{n}  recall {rec / n:.3f}")
    if args.out:
        Path(args.out).write_text(json.dumps(runs, ensure_ascii=False, indent=2), encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
