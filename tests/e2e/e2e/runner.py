"""End-to-end runner: feed recorded phrases through the real pipeline and score the result.

For each phrase the engine receives the audio exactly as a microphone would
deliver it (real-time pacing, trailing silence), and everything it emits is
collected: transcripts, translations, synthesized speech, timings. The checks
then run outside the engine so a bug in the engine cannot hide itself:

  stt          engine transcript vs. the words that were actually said
  ref_asr      Deepgram prerecorded transcript of the same file (is the recording usable?)
  translation  LLM judge (+ reference translation overlap when provided)
  tts          the engine's synthesized speech is transcribed again and compared
               with the translation it was supposed to say
  back         that transcript is translated back and compared with the original
  latency      time from end of speech to the first translation / first audio
"""

from __future__ import annotations

import json
import os
import statistics
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

from . import audio_utils, scoring
from .engine_client import EngineClient, Event
from .providers import ProjectConfig, deepgram_transcribe, pick_groq_model, yandex_translate

SUPPORTED_LANGS = {"ru", "en"}  # the engine ships Piper voices for these two only
RELEVANT_EVENTS = {"transcript", "translation", "tts_audio", "metrics", "error"}


@dataclass
class Thresholds:
    stt_recall: float = 0.80       # share of spoken words found in the engine transcript
    translation_judge: int = 4     # LLM judge, 1-5
    tts_wer: float = 0.30          # WER of re-transcribed speech vs. the translation text
    back_judge: int = 4            # LLM judge on the round trip, 1-5
    # Used instead of the judges when no LLM is reachable (character 3-gram F1, 0-1):
    expected_chrf: float = 0.45    # translation vs. the reference translation
    back_chrf: float = 0.40        # round-trip text vs. what was said
    last_audio_s: float = 15.0     # end of speech -> end of synthesized audio


@dataclass
class Phrase:
    id: str
    direction: str                 # "outgoing" (my language -> theirs) or "incoming"
    audio: Path
    text: Optional[str] = None     # what was said (source language)
    expected: Optional[str] = None  # optional reference translation


@dataclass
class RunOptions:
    out_dir: Path
    engine_exe: Path
    stt_provider: str = "deepgram"
    endpointing_ms: Optional[int] = None
    utterance_end_ms: int = 1000
    repeat: int = 1
    tail_ms: int = 2500
    settle_s: float = 5.0
    max_wait_s: float = 40.0
    skip_ref_asr: bool = False
    skip_back: bool = False
    thresholds: Thresholds = field(default_factory=Thresholds)


def load_manifest(path: Path) -> tuple:
    data = json.loads(path.read_text(encoding="utf-8"))
    langs = data.get("languages", {})
    phrases = []
    for item in data["phrases"]:
        phrases.append(Phrase(
            id=item["id"],
            direction=item["direction"],
            audio=(path.parent / item["audio"]).resolve(),
            text=item.get("text"),
            expected=item.get("expected"),
        ))
    return langs, phrases


def build_engine_env(cfg: ProjectConfig, my_lang: str, their_lang: str, opts: RunOptions) -> dict:
    s = cfg.settings
    env = dict(os.environ)
    models = cfg.models_dir
    en_voice = s.get("tts_outgoing_voice", "en_US-ryan-medium")
    ru_voice = s.get("tts_incoming_voice", "ru_RU-ruslan-medium")
    env.update({
        "DEEPGRAM_API_KEY": cfg.secrets.deepgram,
        "GROQ_API_KEY": cfg.secrets.groq,
        "YANDEX_API_KEY": cfg.secrets.yandex,
        "YANDEX_FOLDER_ID": cfg.env.get("YANDEX_FOLDER_ID") or s.get("yandex_folder_id", ""),
        "TRANSLATOR_PROVIDER": s.get("translation_provider", "auto"),
        "STT_PROVIDER": opts.stt_provider,
        "TRANSLATOR_MODELS_DIR": str(models),
        "TRANSLATOR_TTS_EN_MODEL": str(models / "piper-en" / f"{en_voice}.onnx"),
        "TRANSLATOR_TTS_EN_CONFIG": str(models / "piper-en" / f"{en_voice}.onnx.json"),
        "TRANSLATOR_TTS_RU_MODEL": str(models / "piper-ru" / f"{ru_voice}.onnx"),
        "TRANSLATOR_TTS_RU_CONFIG": str(models / "piper-ru" / f"{ru_voice}.onnx.json"),
        "TRANSLATOR_MY_LANG": my_lang,
        "TRANSLATOR_THEIR_LANG": their_lang,
        "TRANSLATOR_ENDPOINTING_MS": str(opts.endpointing_ms or s.get("endpointing_ms", 300)),
        # Deepgram rejects utterance_end_ms < 1000 with HTTP 400 (checked 2026-10-18),
        # while the app defaults to 500 / 400. Use 1000 so the test exercises Deepgram.
        "TRANSLATOR_UTTERANCE_END_MS": str(opts.utterance_end_ms),
        # Virtual devices: audio comes from inject_audio, output is discarded.
        "TRANSLATOR_MIC_DEVICE": "inject",
        "TRANSLATOR_MEET_INPUT": "inject",
        "TRANSLATOR_SPEAKER_DEVICE": "null",
        "TRANSLATOR_MEET_OUTPUT": "null",
    })
    if cfg.env.get("ORT_DYLIB_PATH"):
        env["ORT_DYLIB_PATH"] = cfg.env["ORT_DYLIB_PATH"]
    return env


class Runner:
    def __init__(self, cfg: ProjectConfig, opts: RunOptions, my_lang: str, their_lang: str):
        if {my_lang, their_lang} - SUPPORTED_LANGS or my_lang == their_lang:
            raise SystemExit(f"unsupported language pair {my_lang}->{their_lang}; engine voices cover ru/en only")
        self.cfg, self.opts = cfg, opts
        self.my_lang, self.their_lang = my_lang, their_lang
        self.results: list = []
        self.llm_ok = self._probe_llm()

    def _probe_llm(self) -> bool:
        """The LLM judge is optional: without it, mechanical metrics decide and the
        semantic questions are left for a human (or the agent) to review."""
        model = pick_groq_model(self.cfg.secrets.groq)
        if model:
            print(f"LLM judge: Groq {model}", flush=True)
            return True
        print("LLM judge unavailable (no Groq model answers); using mechanical metrics, "
              "flagging items for review", flush=True)
        return False

    # -- helpers ----------------------------------------------------------
    def langs_for(self, direction: str) -> tuple:
        if direction == "outgoing":
            return self.my_lang, self.their_lang
        return self.their_lang, self.my_lang

    def log(self, msg: str) -> None:
        print(f"[{time.strftime('%H:%M:%S')}] {msg}", flush=True)

    # -- main entry -------------------------------------------------------
    def run(self, phrases: list) -> list:
        for direction in ("outgoing", "incoming"):
            group = [p for p in phrases if p.direction == direction]
            if group:
                self.run_direction(direction, group)
        return self.results

    def run_direction(self, direction: str, group: list) -> None:
        src_lang, tgt_lang = self.langs_for(direction)
        self.log(f"=== {direction}: {src_lang} -> {tgt_lang}, {len(group)} phrase(s) ===")
        out = self.opts.out_dir
        engine = EngineClient(
            exe=self.opts.engine_exe,
            env=build_engine_env(self.cfg, self.my_lang, self.their_lang, self.opts),
            cwd=out / f"engine_{direction}",
            stderr_log=out / f"engine_{direction}.stderr.log",
        )
        engine.start()
        try:
            problem = self._boot(engine, direction)
            if problem:
                for p in group:
                    self.results.append(self._infra_failure(p, direction, problem))
                self.log(f"engine did not come up: {problem}")
                return
            for rep in range(1, self.opts.repeat + 1):
                for p in group:
                    res = self.run_phrase(engine, p, direction, src_lang, tgt_lang, rep)
                    res["base_id"], res["rep"] = p.id, rep
                    if self.opts.repeat > 1:
                        res["id"] = f"{p.id}#{rep}"
                    self.results.append(res)
        finally:
            engine.shutdown()

    def _boot(self, engine: EngineClient, direction: str) -> Optional[str]:
        engine.send({"cmd": "ping"})
        if engine.wait_for(lambda e: e.kind == "pong", 20) is None:
            return "no pong from engine"
        mark = engine.mark()
        engine.send({"cmd": "start", "pipelines": [direction]})
        ready = engine.wait_for(
            lambda e: e.kind == "error"
            or (e.kind == "tts_status" and e.data.get("direction") == direction
                and e.data.get("status") in ("ok", "fail")),
            120, since=mark,
        )
        if ready is None:
            return "timeout waiting for pipeline/TTS to become ready"
        if ready.kind == "error":
            return f"engine error: {ready.data.get('message', '')[:200]}"
        if ready.data.get("status") != "ok":
            return f"TTS failed to load: {ready.data.get('message', '')[:200]}"
        self.log("pipeline ready (STT connected, TTS loaded)")
        return None

    def _infra_failure(self, p: Phrase, direction: str, reason: str) -> dict:
        return {"id": p.id, "direction": direction, "infra_error": reason, "passed": False,
                "failures": [f"infrastructure: {reason}"]}

    # -- one phrase -------------------------------------------------------
    def run_phrase(self, engine: EngineClient, p: Phrase, direction: str,
                   src_lang: str, tgt_lang: str, rep: int = 1) -> dict:
        o = self.opts
        wav = o.out_dir / "fixtures" / f"{p.id}.wav"
        duration = audio_utils.normalize_to_wav16k(p.audio, wav)
        self.log(f"{p.id}: {duration:.1f}s of audio")

        res = {"id": p.id, "direction": direction, "src_lang": src_lang, "tgt_lang": tgt_lang,
               "audio_s": round(duration, 2), "expected": p.expected, "failures": []}
        res["tts_name"] = p.id if self.opts.repeat == 1 else f"{p.id}#{rep}"

        # Independent reference transcript: tells a bad recording apart from a bad pipeline.
        spoken = p.text
        if not o.skip_ref_asr:
            try:
                res["ref_asr_text"] = deepgram_transcribe(wav, src_lang, self.cfg.secrets.deepgram)
                if spoken:
                    res["ref_asr_wer"] = round(scoring.wer(spoken, res["ref_asr_text"]), 3)
                else:
                    spoken = res["ref_asr_text"]
                    res["reference_source"] = "ref_asr (unverified: no text given)"
            except RuntimeError as e:
                res["ref_asr_error"] = str(e)
        if not spoken:
            res["failures"].append("no reference text and reference ASR unavailable")
            res["passed"] = False
            return res
        res["spoken_text"] = spoken

        # ---- drive the engine ----
        mark = engine.mark()
        engine.send({"cmd": "inject_audio", "pipeline": direction, "path": str(wav), "tail_ms": o.tail_ms})
        started = time.monotonic()
        done = engine.wait_for(
            lambda e: (e.kind == "log" and e.data.get("message", "").startswith("inject_done"))
            or (e.kind == "error" and "inject_audio" in e.data.get("message", "")),
            duration + o.tail_ms / 1000 + 20, since=mark)
        if done is None or done.kind == "error":
            reason = done.data.get("message") if done else "inject did not finish"
            res["failures"].append(f"infrastructure: {reason}")
            res["passed"] = False
            return res

        deadline = time.monotonic() + o.max_wait_s
        while time.monotonic() < deadline:
            if not engine.alive():
                res["failures"].append("engine process died")
                break
            relevant = [e for e in engine.events_since(mark) if e.kind in RELEVANT_EVENTS]
            has_output = any(e.kind in ("translation", "error") for e in relevant)
            idle = time.monotonic() - max((e.ts for e in relevant), default=done.ts)
            if has_output and idle >= o.settle_s:
                break
            time.sleep(0.25)

        events = engine.events_since(mark)
        self._collect(res, events, started)
        self._score(res, p, spoken, src_lang, tgt_lang)
        return res

    def _collect(self, res: dict, events: list, started: float) -> None:
        speech_end = next((e.ts for e in events if e.kind == "log"
                           and e.data.get("message", "").startswith("inject_speech_end")), started)
        transcripts = [e.data["text"] for e in events if e.kind == "transcript"]
        translations = [e for e in events if e.kind == "translation"]
        tts_events = [e for e in events if e.kind == "tts_audio"]
        errors = [e.data.get("message", "") for e in events if e.kind == "error"]

        res["engine_transcripts"] = transcripts
        res["engine_translations"] = [e.data["text"] for e in translations]
        res["engine_errors"] = errors
        res["metrics"] = [{k: e.data.get(k) for k in ("stt_ms", "translate_ms", "tts_ms", "provider_used")}
                          for e in events if e.kind == "metrics"]
        if translations:
            res["first_translation_s"] = round(translations[0].ts - speech_end, 2)
        if tts_events:
            # Translation is streamed, so the first audio can precede the end of speech (negative).
            res["first_audio_s"] = round(tts_events[0].ts - speech_end, 2)
            res["last_audio_s"] = round(tts_events[-1].ts - speech_end, 2)

        res["translation_text"] = " ".join(e.data["text"] for e in translations).strip()
        wav_out = self.opts.out_dir / "tts" / f"{res['tts_name']}.wav"
        res["tts_audio_s"] = round(audio_utils.tts_events_to_wav(tts_events, wav_out), 2)
        if res["tts_audio_s"]:
            res["tts_wav"] = str(wav_out)

    def _score(self, res: dict, p: Phrase, spoken: str, src_lang: str, tgt_lang: str) -> None:
        t, o, key = self.opts.thresholds, self.opts, self.cfg.secrets
        fails = res["failures"]
        review = res.setdefault("needs_review", [])
        translation = res.get("translation_text", "")

        if not res["engine_transcripts"]:
            fails.append("engine produced no transcript")
        else:
            res["stt_recall"] = round(scoring.word_recall(spoken, " ".join(res["engine_transcripts"])), 3)
            if res["stt_recall"] < t.stt_recall:
                fails.append(f"stt recall {res['stt_recall']} < {t.stt_recall}")

        bad = translation.upper().startswith(("ERROR", "TIMEOUT")) or any(
            x.upper().startswith(("ERROR", "TIMEOUT")) for x in res["engine_translations"])
        if not translation or bad:
            fails.append(f"no usable translation ({translation[:60]!r})")
        else:
            if p.expected:
                res["expected_chrf"] = round(scoring.char_ngram_f1(p.expected, translation), 3)
                res["expected_token_f1"] = round(scoring.token_f1(p.expected, translation), 3)
            if self.llm_ok:
                j = scoring.judge_translation(key.groq, src_lang, tgt_lang, spoken, translation, p.expected)
                res["translation_judge"], res["translation_judge_reason"] = j.score, j.reason
                if j.score is None:
                    review.append("translation (judge returned nothing)")
                elif j.score < t.translation_judge:
                    fails.append(f"translation judge {j.score} < {t.translation_judge}")
            elif p.expected:
                if res["expected_chrf"] < t.expected_chrf:
                    fails.append(f"translation chrF vs reference {res['expected_chrf']} < {t.expected_chrf}")
                review.append("translation meaning (no LLM judge; compare with the reference)")
            else:
                review.append("translation meaning (no LLM judge and no reference translation)")

        if not res.get("tts_wav"):
            fails.append("no synthesized audio")
        elif translation and not bad:
            try:
                heard = deepgram_transcribe(Path(res["tts_wav"]), tgt_lang, key.deepgram)
                res["tts_heard"] = heard
                res["tts_wer"] = round(scoring.wer(translation, heard), 3)
                if res["tts_wer"] > t.tts_wer:
                    fails.append(f"tts WER {res['tts_wer']} > {t.tts_wer} (speech not recognised as the translation)")
                if not o.skip_back and heard:
                    self._round_trip(res, spoken, heard, src_lang, tgt_lang, fails, review)
            except RuntimeError as e:
                fails.append(f"tts re-transcription failed: {e}")

        if "last_audio_s" in res and res["last_audio_s"] > t.last_audio_s:
            fails.append(f"audio ended {res['last_audio_s']}s after speech > {t.last_audio_s}s")
        if res["engine_errors"]:
            fails.append(f"engine errors: {res['engine_errors'][:2]}")
        res["passed"] = not fails
        verdict = "PASS" if res["passed"] else "FAIL - " + "; ".join(fails[:3])
        self.log(f"{p.id}: {verdict}" + (" (needs review)" if review and res["passed"] else ""))

    def _round_trip(self, res: dict, spoken: str, heard: str, src_lang: str, tgt_lang: str,
                    fails: list, review: list) -> None:
        t, key, s = self.opts.thresholds, self.cfg.secrets, self.cfg.settings
        try:
            if self.llm_ok:
                back = scoring.back_translate(key.groq, heard, tgt_lang, src_lang)
            else:
                folder = self.cfg.env.get("YANDEX_FOLDER_ID") or s.get("yandex_folder_id", "")
                back = yandex_translate(heard, tgt_lang, src_lang, key.yandex, folder)
        except RuntimeError as e:
            review.append(f"round trip (back-translation failed: {e})")
            return
        res["back_translation"] = back
        res["back_token_f1"] = round(scoring.token_f1(spoken, back), 3)
        res["back_chrf"] = round(scoring.char_ngram_f1(spoken, back), 3)
        if self.llm_ok:
            j = scoring.judge_meaning(key.groq, src_lang, spoken, back)
            res["back_judge"], res["back_judge_reason"] = j.score, j.reason
            if j.score is None:
                review.append("round trip (judge returned nothing)")
            elif j.score < t.back_judge:
                fails.append(f"round-trip judge {j.score} < {t.back_judge}")
        elif res["back_chrf"] < t.back_chrf:
            fails.append(f"round-trip chrF {res['back_chrf']} < {t.back_chrf}")


def summarize(results: list) -> dict:
    def stats(values: list) -> dict:
        if not values:
            return {}
        values = sorted(values)
        p95 = values[min(len(values) - 1, int(round(0.95 * (len(values) - 1))))]
        return {"median": round(statistics.median(values), 2), "p95": round(p95, 2), "max": round(values[-1], 2)}

    total = len(results)
    passed = sum(1 for r in results if r.get("passed"))
    return {
        "total": total,
        "passed": passed,
        "failed": total - passed,
        "infra_errors": sum(1 for r in results if r.get("infra_error")),
        "first_audio_s": stats([r["first_audio_s"] for r in results if "first_audio_s" in r]),
        "last_audio_s": stats([r["last_audio_s"] for r in results if "last_audio_s" in r]),
        "first_translation_s": stats([r["first_translation_s"] for r in results if "first_translation_s" in r]),
        "stt_recall_mean": round(statistics.mean([r["stt_recall"] for r in results if "stt_recall" in r]), 3)
        if any("stt_recall" in r for r in results) else None,
    }
