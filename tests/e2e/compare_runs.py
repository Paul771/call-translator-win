"""Compare finished e2e runs, e.g. one translation provider against another.

    python tests/e2e/compare_runs.py yandex=tests/e2e/results/cmp-yandex groq=tests/e2e/results/cmp-groq
"""

from __future__ import annotations

import json
import statistics
import sys
from pathlib import Path


def load(path: str) -> list:
    return json.loads((Path(path) / "report.json").read_text(encoding="utf-8"))["results"]


def mean(values: list):
    return round(statistics.mean(values), 2) if values else None


def median(values: list):
    return round(statistics.median(values), 2) if values else None


def summarize(results: list) -> dict:
    translate_ms = [m["translate_ms"] for r in results for m in r.get("metrics", []) if m.get("translate_ms") is not None]
    providers = sorted({m.get("provider_used") for r in results for m in r.get("metrics", []) if m.get("provider_used")})
    errors = sum(1 for r in results for t in r.get("engine_translations", []) if t.upper().startswith(("ERROR", "TIMEOUT")))
    return {
        "runs": len(results),
        "passed": sum(1 for r in results if r.get("passed")),
        "translation judge (mean 1-5)": mean([r["translation_judge"] for r in results if r.get("translation_judge")]),
        "round-trip judge (mean 1-5)": mean([r["back_judge"] for r in results if r.get("back_judge")]),
        "round-trip chrF (mean)": mean([r["back_chrf"] for r in results if "back_chrf" in r]),
        "translate_ms median": median(translate_ms),
        "translate_ms max": max(translate_ms) if translate_ms else None,
        "last audio after speech, s (median)": median([r["last_audio_s"] for r in results if "last_audio_s" in r]),
        "translation errors": errors,
        "provider(s) used": ", ".join(providers),
    }


def main() -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    runs = {}
    for arg in sys.argv[1:]:
        label, _, path = arg.partition("=")
        runs[label] = load(path)
    if not runs:
        print(__doc__)
        return 2

    stats = {label: summarize(res) for label, res in runs.items()}
    labels = list(stats)
    print("| metric | " + " | ".join(labels) + " |")
    print("|---|" + "---|" * len(labels))
    for key in next(iter(stats.values())):
        print(f"| {key} | " + " | ".join(str(stats[l][key]) for l in labels) + " |")

    print("\nTranslations of the first repeat (what the engine said):")
    by_phrase = {l: {r.get("base_id", r["id"]): r for r in res if r.get("rep", 1) == 1} for l, res in runs.items()}
    for pid in by_phrase[labels[0]]:
        print(f"\n{pid}: {by_phrase[labels[0]][pid].get('spoken_text')}")
        for l in labels:
            r = by_phrase[l].get(pid)
            if r:
                print(f"  {l:8} {' | '.join(r.get('engine_translations', []))}   [judge {r.get('translation_judge')}]")
    return 0


if __name__ == "__main__":
    sys.exit(main())
