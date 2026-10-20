"""Configuration discovery and the two external services the checks rely on.

Deepgram prerecorded STT (independent reference transcript) and Groq chat
(back-translation and LLM judge). Secrets are read from the project's .env and
settings.json and are never printed.
"""

from __future__ import annotations

import json
import re
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

GROQ_URL = "https://api.groq.com/openai/v1/chat/completions"
GROQ_MODEL = "openai/gpt-oss-120b"
# Groq retires models without notice; the runner picks the first one that answers.
GROQ_CANDIDATES = [
    "openai/gpt-oss-120b", "openai/gpt-oss-20b",
    "llama-3.3-70b-versatile", "llama-3.1-8b-instant",
]
_active = {"model": GROQ_MODEL}
USER_AGENT = "translator-e2e/1.0"

LANG_NAMES = {
    "ru": "Russian", "en": "English", "de": "German", "fr": "French", "es": "Spanish",
    "it": "Italian", "pt": "Portuguese", "uk": "Ukrainian", "pl": "Polish", "tr": "Turkish",
    "zh": "Chinese", "ja": "Japanese", "ko": "Korean", "ar": "Arabic",
}


def find_upwards(start: Path, relative: str) -> Optional[Path]:
    """Walk from `start` up to the filesystem root looking for `relative`."""
    for base in [start, *start.parents]:
        candidate = base / relative
        if candidate.exists():
            return candidate
    return None


def parse_env_file(path: Path) -> dict:
    values = {}
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, value = line.partition("=")
        values[key.strip()] = value.strip().strip('"').strip("'")
    return values


@dataclass
class Secrets:
    deepgram: str = ""
    groq: str = ""
    yandex: str = ""

    def __repr__(self) -> str:  # never leak keys through logs
        return "Secrets(<redacted>)"


@dataclass
class ProjectConfig:
    root: Path
    env: dict = field(default_factory=dict)
    settings: dict = field(default_factory=dict)
    secrets: Secrets = field(default_factory=Secrets)

    @property
    def models_dir(self) -> Path:
        found = find_upwards(self.root, "models/piper-en")
        if found is None:
            raise RuntimeError("models/piper-en not found above the repo (Piper TTS models are required)")
        return found.parent.parent / "models"


def load_project(start: Path, env_file: Optional[Path] = None) -> ProjectConfig:
    env_path = env_file or find_upwards(start, ".env")
    settings_path = find_upwards(start, "settings.json")
    env = parse_env_file(env_path) if env_path else {}
    settings = {}
    if settings_path:
        settings = json.loads(settings_path.read_text(encoding="utf-8"))
    cfg = ProjectConfig(root=start, env=env, settings=settings)
    # Same precedence as the app (web/settings.py, Elixir): settings.json wins over .env.
    cfg.secrets = Secrets(
        deepgram=settings.get("deepgram_api_key") or env.get("DEEPGRAM_API_KEY", ""),
        groq=settings.get("groq_api_key") or env.get("GROQ_API_KEY", ""),
        yandex=settings.get("yandex_api_key") or env.get("YANDEX_API_KEY", ""),
    )
    return cfg


# ---------------------------------------------------------------------------
# Deepgram prerecorded STT
# ---------------------------------------------------------------------------

def deepgram_transcribe(wav_path: Path, language: str, key: str, timeout: int = 90) -> str:
    """Transcribe a WAV with Deepgram's prerecorded API. Returns the transcript text."""
    if not key:
        raise RuntimeError("no Deepgram key")
    body = wav_path.read_bytes()
    last_error = None
    for model in ("nova-3", "nova-2"):
        url = (
            "https://api.deepgram.com/v1/listen"
            f"?model={model}&language={language}&smart_format=true&punctuate=true"
        )
        req = urllib.request.Request(
            url,
            data=body,
            headers={
                "Authorization": f"Token {key}",
                "Content-Type": "audio/wav",
                "User-Agent": USER_AGENT,
            },
        )
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                data = json.loads(resp.read().decode("utf-8"))
            alt = data["results"]["channels"][0]["alternatives"][0]
            transcript = alt.get("transcript", "").strip()
            if not transcript and model == "nova-3":
                last_error = "empty transcript on nova-3"
                continue  # nova-3 sometimes returns nothing for very short clips
            return transcript
        except urllib.error.HTTPError as e:
            last_error = f"HTTP {e.code} on {model}"
            if e.code not in (400, 422):
                break
        except (urllib.error.URLError, TimeoutError, KeyError, IndexError) as e:
            last_error = f"{type(e).__name__} on {model}"
            break
    raise RuntimeError(f"Deepgram transcription failed: {last_error}")


# ---------------------------------------------------------------------------
# Groq chat
# ---------------------------------------------------------------------------

def groq_chat(messages: list, key: str, temperature: float = 0.0, max_tokens: int = 300,
              json_mode: bool = False, retries: int = 4, timeout: int = 30,
              model: Optional[str] = None, reasoning_effort: str = "low") -> str:
    if not key:
        raise RuntimeError("no Groq key")
    model = model or _active["model"]
    body = {
        "model": model,
        "messages": messages,
        "temperature": temperature,
        "max_tokens": max_tokens,
    }
    if model.startswith("openai/gpt-oss"):
        # Reasoning models spend completion tokens on thinking; leave room for the answer.
        body["reasoning_effort"] = reasoning_effort
        body["max_tokens"] = max(max_tokens, 600)
    if json_mode:
        body["response_format"] = {"type": "json_object"}
    payload = json.dumps(body).encode("utf-8")
    for attempt in range(retries):
        req = urllib.request.Request(
            GROQ_URL,
            data=payload,
            headers={
                "Authorization": f"Bearer {key}",
                "Content-Type": "application/json",
                "User-Agent": USER_AGENT,
            },
        )
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                data = json.loads(resp.read().decode("utf-8"))
            content = (data["choices"][0]["message"].get("content") or "").strip()
            if not content:
                raise RuntimeError("Groq returned an empty reply")
            return content
        except urllib.error.HTTPError as e:
            err_body = e.read().decode("utf-8", "replace")
            # Groq sometimes answers a valid request with a bare {"error":{"message":"Not Found"}}
            # and succeeds a moment later. A real missing model says "does not exist".
            transient_404 = e.code == 404 and "does not exist" not in err_body
            if (e.code == 429 or transient_404) and attempt < retries - 1:
                wait = float(e.headers.get("Retry-After", 2 ** attempt * 2)) if e.code == 429 else 1.5
                time.sleep(min(wait, 30))
                continue
            raise RuntimeError(f"Groq HTTP {e.code}") from None
        except (urllib.error.URLError, TimeoutError) as e:
            if attempt < retries - 1:
                time.sleep(2 ** attempt)
                continue
            raise RuntimeError(f"Groq request failed: {type(e).__name__}") from None
    raise RuntimeError("Groq: retries exhausted")


def pick_groq_model(key: str) -> Optional[str]:
    """Select the first candidate model that answers; remembers it for groq_chat."""
    for model in GROQ_CANDIDATES:
        _active["model"] = model
        try:
            groq_chat([{"role": "user", "content": "Reply with the single word OK."}],
                      key, max_tokens=16, retries=4, timeout=30)
            return model
        except RuntimeError:
            continue
    return None


def yandex_translate(text: str, source: str, target: str, key: str, folder_id: str,
                     timeout: int = 20) -> str:
    """Machine translation used for the round trip when no LLM is reachable."""
    if not (key and folder_id):
        raise RuntimeError("no Yandex key/folder")
    body = json.dumps({
        "sourceLanguageCode": source, "targetLanguageCode": target,
        "texts": [text], "folderId": folder_id,
    }).encode("utf-8")
    req = urllib.request.Request(
        "https://translate.api.cloud.yandex.net/translate/v2/translate",
        data=body,
        headers={"Authorization": f"Api-Key {key}", "Content-Type": "application/json",
                 "User-Agent": USER_AGENT},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode("utf-8"))["translations"][0]["text"]
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"Yandex HTTP {e.code}") from None
    except (urllib.error.URLError, TimeoutError, KeyError, IndexError) as e:
        raise RuntimeError(f"Yandex request failed: {type(e).__name__}") from None


def parse_json_object(text: str) -> dict:
    """Extract the first JSON object from an LLM reply."""
    match = re.search(r"\{.*\}", text, re.S)
    if not match:
        raise ValueError(f"no JSON object in reply: {text[:80]!r}")
    return json.loads(match.group(0))
