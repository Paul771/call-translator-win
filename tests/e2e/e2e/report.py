"""Write report.json and report.md for a finished run."""

from __future__ import annotations

import json
from pathlib import Path

from .runner import Thresholds, summarize


def _cell(value, suffix: str = "") -> str:
    return "-" if value is None else f"{value}{suffix}"


def write_reports(out_dir: Path, results: list, thresholds: Thresholds, meta: dict) -> dict:
    summary = summarize(results)
    (out_dir / "report.json").write_text(
        json.dumps({"meta": meta, "summary": summary, "results": results}, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )

    lines = [
        "# End-to-end translation test",
        "",
        f"- Run: `{meta['started']}`  |  engine: `{meta['engine']}`  |  STT: `{meta['stt_provider']}`",
        f"- Languages: my=`{meta['my_lang']}`, their=`{meta['their_lang']}`",
        f"- **Result: {summary['passed']}/{summary['total']} passed**"
        + (f", {summary['infra_errors']} infrastructure error(s)" if summary["infra_errors"] else ""),
        f"- Last audio after end of speech (s): {summary['last_audio_s'] or '-'}",
        f"- First audio relative to end of speech (s, negative = streamed before the speaker finished): "
        f"{summary['first_audio_s'] or '-'}",
        "",
        "Thresholds: " + ", ".join(f"{k}={v}" for k, v in vars(thresholds).items()),
        "",
        "| id | dir | verdict | stt recall | ref ASR WER | transl. chrF | TTS WER | round-trip chrF | last audio s |",
        "|----|-----|---------|-----------:|------------:|------------:|--------:|----------------:|-------------:|",
    ]
    for r in results:
        lines.append(
            f"| {r['id']} | {r['direction']} | {'PASS' if r.get('passed') else 'FAIL'} "
            f"| {_cell(r.get('stt_recall'))} | {_cell(r.get('ref_asr_wer'))} "
            f"| {_cell(r.get('expected_chrf'))} | {_cell(r.get('tts_wer'))} "
            f"| {_cell(r.get('back_chrf'))} | {_cell(r.get('last_audio_s'))} |"
        )

    if any(r.get("rep", 1) > 1 for r in results):
        rates = {}
        for r in results:
            ok, total = rates.get(r["base_id"], (0, 0))
            rates[r["base_id"]] = (ok + bool(r.get("passed")), total + 1)
        lines += ["", "## Pass rate by phrase (repeated runs)", "", "| phrase | passed | verdict |", "|--------|-------:|---------|"]
        for pid, (ok, total) in rates.items():
            verdict = "stable pass" if ok == total else ("stable FAIL" if ok == 0 else "flaky")
            lines.append(f"| {pid} | {ok}/{total} | {verdict} |")

    pending = [r for r in results if r.get("needs_review")]
    if pending:
        lines += ["", "## Needs review (no automatic verdict)", ""]
        lines += [f"- **{r['id']}**: " + "; ".join(r["needs_review"]) for r in pending]

    lines += ["", "## Details", ""]
    for r in results:
        lines.append(f"### {r['id']} ({r['direction']}) - {'PASS' if r.get('passed') else 'FAIL'}")
        if r.get("infra_error"):
            lines += [f"- infrastructure: {r['infra_error']}", ""]
            continue
        rows = [
            ("said", r.get("spoken_text")),
            ("reference ASR", r.get("ref_asr_text")),
            ("engine transcript", " | ".join(r.get("engine_transcripts", []))),
            ("translation", r.get("translation_text")),
            ("reference translation", r.get("expected")),
            ("judge", f"{r.get('translation_judge')} - {r.get('translation_judge_reason', '')}"
             if "translation_judge" in r else None),
            ("speech re-heard", r.get("tts_heard")),
            ("back-translation", r.get("back_translation")),
            ("round-trip judge", f"{r.get('back_judge')} - {r.get('back_judge_reason', '')}"
             if "back_judge" in r else None),
        ]
        lines += [f"- **{name}:** {value}" for name, value in rows if value]
        if r.get("metrics"):
            lines.append("- engine timings (ms): " + "; ".join(
                f"stt={m['stt_ms']} translate={m['translate_ms']} tts={m['tts_ms']} via {m['provider_used']}"
                for m in r["metrics"]))
        if r.get("failures"):
            lines.append("- **failures:** " + "; ".join(r["failures"]))
        lines.append("")

    (out_dir / "report.md").write_text("\n".join(lines), encoding="utf-8")
    return summary
