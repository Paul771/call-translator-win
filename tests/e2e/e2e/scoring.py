"""Text comparison metrics and LLM-based judges. No third-party dependencies."""

from __future__ import annotations

import re
from collections import Counter
from dataclasses import dataclass
from typing import Optional

from .providers import LANG_NAMES, groq_chat, parse_json_object


# ---------------------------------------------------------------------------
# Normalisation and string metrics
# ---------------------------------------------------------------------------

def normalize(text: str) -> str:
    text = text.lower().replace("ё", "е")
    text = re.sub(r"[^\w\s]", " ", text, flags=re.UNICODE).replace("_", " ")
    return re.sub(r"\s+", " ", text).strip()


def tokens(text: str) -> list:
    n = normalize(text)
    return n.split() if n else []


def _edit_distance(a: list, b: list) -> int:
    prev = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        cur = [i]
        for j, y in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (x != y)))
        prev = cur
    return prev[-1]


def wer(reference: str, hypothesis: str) -> float:
    ref, hyp = tokens(reference), tokens(hypothesis)
    if not ref:
        return 0.0 if not hyp else 1.0
    return _edit_distance(ref, hyp) / len(ref)


def word_recall(reference: str, hypothesis: str) -> float:
    """Share of reference words present in the hypothesis (order ignored)."""
    ref, hyp = Counter(tokens(reference)), Counter(tokens(hypothesis))
    total = sum(ref.values())
    if total == 0:
        return 1.0
    return sum((ref & hyp).values()) / total


def token_f1(a: str, b: str) -> float:
    ca, cb = Counter(tokens(a)), Counter(tokens(b))
    common = sum((ca & cb).values())
    if common == 0:
        return 0.0
    precision = common / sum(cb.values())
    recall = common / sum(ca.values())
    return 2 * precision * recall / (precision + recall)


def char_ngram_f1(a: str, b: str, n: int = 3) -> float:
    """chrF-like score: robust to inflection, which matters for Russian."""
    def grams(s: str) -> Counter:
        s = normalize(s).replace(" ", "_")
        return Counter(s[i:i + n] for i in range(max(len(s) - n + 1, 0)))

    ga, gb = grams(a), grams(b)
    common = sum((ga & gb).values())
    if common == 0:
        return 0.0
    precision = common / sum(gb.values())
    recall = common / sum(ga.values())
    return 2 * precision * recall / (precision + recall)


# ---------------------------------------------------------------------------
# LLM judge / back-translation (Groq)
# ---------------------------------------------------------------------------

@dataclass
class Judgement:
    score: Optional[int]
    reason: str


_JUDGE_SYSTEM = (
    "You are a strict bilingual evaluator of live-interpreter output. "
    "Reply with a single JSON object: {\"score\": <integer 1-5>, \"reason\": \"<one short sentence>\"}. "
    "Rubric: 5 = faithful and natural; 4 = minor wording differences, same meaning; "
    "3 = partly wrong or something missing; 2 = major meaning errors; 1 = unrelated or empty. "
    "Ignore punctuation and capitalisation. Do not reward fluent text that changes the meaning."
)


def judge_translation(key: str, src_lang: str, tgt_lang: str, source_text: str,
                      translation: str, reference: Optional[str] = None) -> Judgement:
    lines = [
        f"Source language: {LANG_NAMES.get(src_lang, src_lang)}",
        f"Target language: {LANG_NAMES.get(tgt_lang, tgt_lang)}",
        f"Source text: {source_text}",
        f"Translation under test: {translation}",
    ]
    if reference:
        lines.append(f"Reference translation (one acceptable option): {reference}")
    return _ask_judge(key, "\n".join(lines))


def judge_meaning(key: str, lang: str, original: str, other: str) -> Judgement:
    """Compare two texts in the same language for preserved meaning."""
    text = (
        f"Language: {LANG_NAMES.get(lang, lang)}\n"
        f"Original sentence: {original}\n"
        f"Sentence after a translate-and-back round trip: {other}\n"
        "Score how well the second sentence preserves the meaning of the first."
    )
    return _ask_judge(key, text)


def _ask_judge(key: str, user_text: str) -> Judgement:
    try:
        reply = groq_chat(
            [{"role": "system", "content": _JUDGE_SYSTEM}, {"role": "user", "content": user_text}],
            key, temperature=0.0, max_tokens=120, json_mode=True,
        )
        data = parse_json_object(reply)
        score = int(data["score"])
        if not 1 <= score <= 5:
            raise ValueError(f"score out of range: {score}")
        return Judgement(score, str(data.get("reason", "")).strip())
    except (RuntimeError, ValueError, KeyError, TypeError) as e:
        return Judgement(None, f"judge unavailable: {e}")


def back_translate(key: str, text: str, from_lang: str, to_lang: str) -> str:
    system = (
        f"You are a literal interpreter. Translate the user's {LANG_NAMES.get(from_lang, from_lang)} "
        f"text into {LANG_NAMES.get(to_lang, to_lang)}. Output only the translation."
    )
    return groq_chat(
        [{"role": "system", "content": system}, {"role": "user", "content": text}],
        key, temperature=0.0, max_tokens=200,
    )
