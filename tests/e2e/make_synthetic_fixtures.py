"""Generate synthetic test phrases with the Windows SAPI voices.

Lets the whole harness be verified without anyone recording anything. The voices
are clean and robotic, so results are a ceiling for real speech, not a substitute.

    python tests/e2e/make_synthetic_fixtures.py
    -> tests/e2e/fixtures/synthetic/*.wav and tests/e2e/phrases.synthetic.json
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "fixtures" / "synthetic"

# (id, direction, voice, spoken text, reference translation)
PHRASES = [
    ("ru-01", "outgoing", "Microsoft Irina Desktop",
     "Добрый день, меня зовут Павел, рад познакомиться.",
     "Good afternoon, my name is Pavel, nice to meet you."),
    ("ru-02", "outgoing", "Microsoft Irina Desktop",
     "Можете повторить, пожалуйста, я не расслышал последнее предложение.",
     "Could you please repeat that, I didn't catch the last sentence."),
    ("ru-03", "outgoing", "Microsoft Irina Desktop",
     "Давайте перенесём встречу на пятницу в три часа дня.",
     "Let's move the meeting to Friday at three in the afternoon."),
    ("ru-04", "outgoing", "Microsoft Irina Desktop",
     "Мы отправили вам коммерческое предложение по электронной почте.",
     "We sent you the commercial proposal by email."),
    ("ru-05", "outgoing", "Microsoft Irina Desktop",
     "Спасибо за звонок, до свидания.",
     "Thank you for calling, goodbye."),
    ("en-01", "incoming", "Microsoft David Desktop",
     "Hello, can you hear me clearly?",
     "Здравствуйте, вы хорошо меня слышите?"),
    ("en-02", "incoming", "Microsoft David Desktop",
     "Our team will finish the project by the end of next month.",
     "Наша команда закончит проект к концу следующего месяца."),
    ("en-03", "incoming", "Microsoft Zira Desktop",
     "I would like to discuss the budget for the next quarter.",
     "Я хотел бы обсудить бюджет на следующий квартал."),
    ("en-04", "incoming", "Microsoft Zira Desktop",
     "Please send me the contract before Thursday.",
     "Пожалуйста, отправьте мне договор до четверга."),
    ("en-05", "incoming", "Microsoft David Desktop",
     "That sounds great, let's talk again tomorrow.",
     "Отлично, давайте поговорим снова завтра."),
]

PS_SCRIPT = r"""
param([string]$JsonPath, [string]$OutDir)
Add-Type -AssemblyName System.Speech
$items = Get-Content -Raw -Encoding UTF8 $JsonPath | ConvertFrom-Json
foreach ($it in $items) {
    $s = New-Object System.Speech.Synthesis.SpeechSynthesizer
    $s.SelectVoice($it.voice)
    $s.SetOutputToWaveFile((Join-Path $OutDir ($it.id + '.raw.wav')))
    $s.Speak($it.text)
    $s.Dispose()
}
"""


def main() -> int:
    shell = shutil.which("pwsh") or shutil.which("powershell")
    if not shell:
        print("PowerShell not found")
        return 2
    OUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        (tmp_path / "items.json").write_text(
            json.dumps([{"id": i, "voice": v, "text": t} for i, _, v, t, _ in PHRASES], ensure_ascii=False),
            encoding="utf-8")
        (tmp_path / "speak.ps1").write_text(PS_SCRIPT, encoding="utf-8-sig")
        r = subprocess.run(
            [shell, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(tmp_path / "speak.ps1"),
             "-JsonPath", str(tmp_path / "items.json"), "-OutDir", str(OUT)],
            capture_output=True, text=True)
        if r.returncode != 0:
            print("speech synthesis failed:", r.stderr.strip() or r.stdout.strip())
            return 2

    sys.path.insert(0, str(HERE))
    from e2e.audio_utils import normalize_to_wav16k

    manifest = {"languages": {"my": "ru", "their": "en"}, "phrases": []}
    for pid, direction, _voice, text, expected in PHRASES:
        raw = OUT / f"{pid}.raw.wav"
        final = OUT / f"{pid}.wav"
        normalize_to_wav16k(raw, final)
        raw.unlink()
        manifest["phrases"].append({
            "id": pid, "direction": direction,
            "audio": f"fixtures/synthetic/{pid}.wav",
            "text": text, "expected": expected,
        })
    target = HERE / "phrases.synthetic.json"
    target.write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"wrote {len(PHRASES)} phrases -> {target}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
