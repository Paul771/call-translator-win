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


def call_yandex(text, source_lang, target_lang, api_key, folder_id, timeout=10):
    """Translate text via Yandex Translate API."""
    body = json.dumps({
        "sourceLanguageCode": source_lang,
        "targetLanguageCode": target_lang,
        "texts": [text],
        "folderId": folder_id,
    }).encode()
    req = urllib.request.Request(
        "https://translate.api.cloud.yandex.net/translate/v2/translate",
        data=body,
        headers={
            "Authorization": f"Api-Key {api_key}",
            "Content-Type": "application/json",
            "User-Agent": USER_AGENT,
        },
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        result = json.loads(resp.read().decode())
    return result["translations"][0]["text"]


def get_yandex_key():
    from .settings import load_settings
    settings = load_settings()
    return settings.get("yandex_api_key", "") or os.environ.get("YANDEX_API_KEY", "")


def get_yandex_folder_id():
    from .settings import load_settings
    settings = load_settings()
    return settings.get("yandex_folder_id", "") or os.environ.get("YANDEX_FOLDER_ID", "")


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
            return _list_audio_devices_windows()
        else:
            return _list_audio_devices_macos()
    except Exception as e:
        _logger = logging.getLogger('translator')
        _logger.error(f"Failed to list audio devices: {e}")
        return {"input": [], "output": []}


def _list_audio_devices_windows():
    """Enumerate audio devices via WASAPI using pycaw.

    Returns separate lists for capture (microphones) and render (speakers/headphones).
    This sees ALL device types including Bluetooth, USB-C, and virtual audio devices
    that Win32_SoundDevice (WMI) misses.
    """
    _logger = logging.getLogger('translator')

    try:
        from pycaw.pycaw import AudioUtilities, EDataFlow, AudioDeviceState
    except ImportError:
        _logger.warning("pycaw not installed. Install with: pip install pycaw")
        return {"input": [], "output": []}

    input_devices = []
    output_devices = []

    try:
        # Initialize COM for this thread (required by pycaw/WASAPI)
        import ctypes as _ctypes
        _ole32 = _ctypes.windll.ole32
        _hr = _ole32.CoInitializeEx(None, 0)  # COINIT_APARTMENTTHREADED
        if _hr < 0 and _hr != -2147417850:  # RPC_E_CHANGED_MODE is OK
            _logger.warning(f"CoInitializeEx failed: hr={_hr:#x}")
        else:
            _logger.debug(f"CoInitializeEx OK: hr={_hr:#x}")
    except Exception as _com_err:
        _logger.warning(f"CoInitializeEx error: {_com_err}")

    try:
        # eCapture = 1 (microphones), eRender = 0 (speakers/headphones)
        for data_flow, label in [
            (EDataFlow.eCapture.value, "input"),
            (EDataFlow.eRender.value, "output"),
        ]:
            devices = AudioUtilities.GetAllDevices(
                data_flow=data_flow,
            )
            _logger.info(f"WASAPI {label}: GetAllDevices returned {len(devices)} devices")

            result = []
            for i, d in enumerate(devices):
                try:
                    name = d.FriendlyName
                    state = d.state
                    if name and state == AudioDeviceState.Active:
                        result.append(name)
                        _logger.debug(f"  [{i}] ACTIVE: {name}")
                    elif name:
                        _logger.debug(f"  [{i}] state={state}: {name}")
                    else:
                        _logger.debug(f"  [{i}] state={state}: <no name>")
                except Exception as ex:
                    _logger.debug(f"  [{i}] error: {ex}")
                    continue

            _logger.info(f"WASAPI {label}: {len(result)} active devices with names")
            sorted_result = sorted(set(result))

            if label == "input":
                input_devices = sorted_result
            else:
                output_devices = sorted_result

    except Exception as e:
        _logger.error(f"WASAPI enumeration failed: {e}", exc_info=True)

    return {"input": input_devices, "output": output_devices}


def _list_audio_devices_macos():
    """Enumerate audio devices on macOS via system_profiler."""
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
    sorted_devices = sorted(devices)
    return {"input": sorted_devices, "output": sorted_devices}
