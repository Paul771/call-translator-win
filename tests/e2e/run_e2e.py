"""Run the end-to-end translation test.

    python tests/e2e/run_e2e.py --phrases tests/e2e/phrases.synthetic.json
    python tests/e2e/run_e2e.py --phrases tests/e2e/phrases.json --only ru-01 en-02

Exit code: 0 all passed, 1 some phrases failed, 2 infrastructure problem.
"""

from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from e2e.providers import load_project  # noqa: E402
from e2e.report import write_reports  # noqa: E402
from e2e.runner import RunOptions, Runner, Thresholds, load_manifest  # noqa: E402


def find_engine(repo: Path, explicit: str | None) -> Path:
    if explicit:
        return Path(explicit).resolve()
    for rel in ("native/audio_engine/target/release/audio_engine.exe",
                "native/audio_engine/target/debug/audio_engine.exe"):
        if (repo / rel).exists():
            return repo / rel
    raise SystemExit("audio_engine.exe not built: run `cargo build --release --bin audio_engine` in native/audio_engine")


def main() -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    repo = HERE.parent.parent
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--phrases", required=True, help="phrases manifest (JSON)")
    ap.add_argument("--only", nargs="*", help="run only these phrase ids")
    ap.add_argument("--direction", choices=["outgoing", "incoming"], help="run only one direction")
    ap.add_argument("--engine", help="path to audio_engine.exe (default: release, then debug build)")
    ap.add_argument("--env-file", help=".env with API keys (default: searched upwards from the repo)")
    ap.add_argument("--stt-provider", default="deepgram", choices=["deepgram", "yandex", "whisper", "auto"],
                    help="STT provider for the engine (default deepgram: no silent fallback)")
    ap.add_argument("--endpointing-ms", type=int, help="override Deepgram endpointing")
    ap.add_argument("--utterance-end-ms", type=int, default=1000,
                    help="Deepgram utterance_end_ms (default 1000; values below 1000 are rejected by the API)")
    ap.add_argument("--repeat", type=int, default=1,
                    help="run every phrase N times: streaming STT varies, repeats separate flaky from systematic")
    ap.add_argument("--translation-provider", choices=["auto", "groq", "yandex", "litellm"],
                    help="force one translation provider (default: from settings.json, normally auto)")
    ap.add_argument("--groq-model", help="Groq model for the engine (GROQ_MODEL)")
    ap.add_argument("--skip-ref-asr", action="store_true", help="skip the Deepgram reference transcript")
    ap.add_argument("--skip-back", action="store_true", help="skip the back-translation check")
    ap.add_argument("--out", help="output directory (default tests/e2e/results/<timestamp>)")
    args = ap.parse_args()

    cfg = load_project(repo, Path(args.env_file) if args.env_file else None)
    missing = [n for n, v in (("DEEPGRAM_API_KEY", cfg.secrets.deepgram), ("GROQ_API_KEY", cfg.secrets.groq)) if not v]
    if missing:
        print(f"missing API keys: {', '.join(missing)} (looked in .env and settings.json)")
        return 2

    manifest = Path(args.phrases).resolve()
    langs, phrases = load_manifest(manifest)
    if args.only:
        phrases = [p for p in phrases if p.id in args.only]
    if args.direction:
        phrases = [p for p in phrases if p.direction == args.direction]
    if not phrases:
        print("no phrases selected")
        return 2
    missing_audio = [str(p.audio) for p in phrases if not p.audio.exists()]
    if missing_audio:
        print("audio files not found:\n  " + "\n  ".join(missing_audio))
        return 2

    my_lang = langs.get("my") or cfg.settings.get("my_language", "ru")
    their_lang = langs.get("their") or cfg.settings.get("their_language", "en")
    stamp = time.strftime("%Y%m%d-%H%M%S")
    out_dir = (Path(args.out) if args.out else HERE / "results" / stamp).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    opts = RunOptions(
        out_dir=out_dir,
        engine_exe=find_engine(repo, args.engine),
        stt_provider=args.stt_provider,
        endpointing_ms=args.endpointing_ms,
        utterance_end_ms=args.utterance_end_ms,
        repeat=max(1, args.repeat),
        translation_provider=args.translation_provider,
        groq_model=args.groq_model,
        skip_ref_asr=args.skip_ref_asr,
        skip_back=args.skip_back,
        thresholds=Thresholds(),
    )
    runner = Runner(cfg, opts, my_lang, their_lang)
    results = runner.run(phrases)

    meta = {"started": stamp, "engine": str(opts.engine_exe.relative_to(repo)) if opts.engine_exe.is_relative_to(repo)
            else str(opts.engine_exe), "stt_provider": args.stt_provider, "my_lang": my_lang, "their_lang": their_lang}
    summary = write_reports(out_dir, results, opts.thresholds, meta)
    print(f"\n{summary['passed']}/{summary['total']} passed - report: {out_dir / 'report.md'}")

    if summary["infra_errors"]:
        return 2
    return 0 if summary["failed"] == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
