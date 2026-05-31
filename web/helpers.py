"""Helper functions: Groq API, engine commands, voice catalog, audio devices."""

import os
import json
import glob
import logging
import socket
import subprocess
import urllib.request
from collections import defaultdict

from .settings import (
    GROQ_MODEL, GROQ_CHAT_URL, PIPER_VOICES_URL,
    USER_AGENT, CMD_HOST, CMD_PORT, MODELS_DIR,
)


def call_groq(messages, api_key, temperature=0.1, max_tokens=None, timeout=10):
    body = {"model": GROQ_MODEL, "messages": messages, "temperature": temperature}
    if max_tokens:
        body["max_tokens"] = max_tokens
    req = urllib.request.Request(
        GROQ_CHAT_URL,
        data=json.dumps(body).encode(),
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
            "User-Agent": USER_AGENT,
        },
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        result = json.loads(resp.read().decode())
    return result["choices"][0]["message"]["content"].strip()


def send_engine_command(cmd, timeout=10):
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.settimeout(timeout)
        s.connect((CMD_HOST, CMD_PORT))
        s.send((cmd + "\n").encode())
        # Read full response (may be large for audio data)
        chunks = []
        while True:
            try:
                chunk = s.recv(65536)
                if not chunk:
                    break
                chunks.append(chunk)
            except socket.timeout:
                break
        s.close()
        return b"".join(chunks).decode().strip()
    except Exception as e:
        return f"error:{e}"


# Piper voice catalog -- fetched once at startup, cached
_voice_catalog = None


def get_voice_catalog():
    """Fetch and cache the full Piper voices.json from HuggingFace."""
    global _voice_catalog
    if _voice_catalog is not None:
        return _voice_catalog
    try:
        req = urllib.request.Request(
            f"{PIPER_VOICES_URL}/voices.json",
            headers={"User-Agent": USER_AGENT},
        )
        data = json.loads(urllib.request.urlopen(req, timeout=30).read())
        catalog = defaultdict(list)
        for key, info in data.items():
            family = info["language"]["family"]
            files = info.get("files", {})
            total_size = sum(f.get("size_bytes", 0) for f in files.values())
            file_list = []
            for fpath in files:
                file_list.append({
                    "url": f"{PIPER_VOICES_URL}/{fpath}",
                    "path": fpath.split("/")[-1],
                    "size": files[fpath].get("size_bytes", 0),
                })
            catalog[family].append({
                "name": key,
                "quality": info.get("quality", ""),
                "size": total_size,
                "files": file_list,
            })
        _voice_catalog = dict(catalog)
    except Exception:
        _voice_catalog = {}
    return _voice_catalog


def scan_voices():
    voices = {}
    for d in sorted(glob.glob(os.path.join(MODELS_DIR, "piper-*"))):
        lang = os.path.basename(d).replace("piper-", "")
        voice_list = []
        for onnx in sorted(glob.glob(os.path.join(d, "*.onnx"))):
            voice_list.append(os.path.basename(onnx).replace(".onnx", ""))
        if voice_list:
            voices[lang] = voice_list
    return voices


def list_audio_devices():
    try:
        import platform
        if platform.system() == "Windows":
            # Use PowerShell with UTF-8 output encoding to get Windows audio devices.
            # $OutputEncoding controls the encoding used when piping to external programs
            # (i.e. how subprocess.run captures stdout). [Console]::OutputEncoding alone
            # is not sufficient — we must set both.
            cmd = [
                "powershell", "-NoProfile", "-Command",
                "$OutputEncoding = [Text.UTF8Encoding]::UTF8; " +
                "[Console]::OutputEncoding = [Text.UTF8Encoding]::UTF8; " +
                "Get-CimInstance Win32_SoundDevice | Select-Object -ExpandProperty Name"
            ]
            r = subprocess.run(cmd, capture_output=True, timeout=10)
            # Try decoding with multiple encodings common on Russian Windows systems
            raw = r.stdout
            for enc in ('utf-8-sig', 'utf-8', 'cp1251', 'cp866', 'cp1252'):
                try:
                    stdout = raw.decode(enc).strip()
                    if stdout and any(
                        'Ѐ' <= c <= 'ӿ'  # Cyrillic range check
                        for c in stdout
                    ):
                        break
                except (UnicodeDecodeError, LookupError):
                    continue
            else:
                for enc in ('utf-8-sig', 'utf-8', 'cp1251', 'cp866', 'cp1252'):
                    try:
                        stdout = raw.decode(enc).strip()
                        break
                    except (UnicodeDecodeError, LookupError):
                        continue
                else:
                    stdout = raw.decode('utf-8', errors='replace').strip()
            if r.returncode == 0:
                devices = [line.strip() for line in stdout.split('\n') if line.strip()]
                devices = [d for d in devices if d and not d.startswith('PS ')]
                return sorted(devices)
        else:
            r = subprocess.run(
                ["system_profiler", "SPAudioDataType", "-json"],
                capture_output=True, text=True, timeout=5,
            )
            data = json.loads(r.stdout)
            devices = set()
            for section in data.get("SPAudioDataType", []):
                for item in section.get("_items", []):
                    name = item.get("_name", "")
                    if name:
                        devices.add(name)
            return sorted(devices)
    except Exception as e:
        logger = logging.getLogger('translator')
        logger.error(f"Failed to list audio devices: {e}")
        return []
