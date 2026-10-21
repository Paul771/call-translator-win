"""Drive audio_engine.exe over its length-prefixed JSON stdin/stdout protocol.

Wire format (see native/audio_engine/src/protocol.rs): 4-byte big-endian length
followed by a JSON payload, in both directions. Every event is stamped with a
monotonic arrival time so the runner can compute latencies.
"""

from __future__ import annotations

import json
import re
import struct
import subprocess
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Optional

# The engine prints key prefixes to stderr; never let them reach saved logs.
_KEY_LINE = re.compile(r"(\[ENGINE\] \w*API_KEY from env:).*")


@dataclass
class Event:
    ts: float  # time.monotonic() at arrival
    data: dict

    @property
    def kind(self) -> str:
        return self.data.get("event", "")


@dataclass
class EngineClient:
    exe: Path
    env: dict
    cwd: Path
    stderr_log: Path
    events: list = field(default_factory=list)
    _proc: Optional[subprocess.Popen] = None
    _cond: threading.Condition = field(default_factory=threading.Condition)
    _reader: Optional[threading.Thread] = None
    _err_reader: Optional[threading.Thread] = None
    _eof: bool = False

    def start(self) -> None:
        self.cwd.mkdir(parents=True, exist_ok=True)
        self._proc = subprocess.Popen(
            [str(self.exe)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=str(self.cwd),
            env=self.env,
            bufsize=0,
        )
        self._reader = threading.Thread(target=self._read_events, daemon=True)
        self._reader.start()
        self._err_reader = threading.Thread(target=self._read_stderr, daemon=True)
        self._err_reader.start()

    # -- reading ---------------------------------------------------------
    def _read_exact(self, n: int) -> Optional[bytes]:
        buf = b""
        while len(buf) < n:
            chunk = self._proc.stdout.read(n - len(buf))
            if not chunk:
                return None
            buf += chunk
        return buf

    def _read_events(self) -> None:
        while True:
            header = self._read_exact(4)
            if header is None:
                break
            (length,) = struct.unpack(">I", header)
            payload = self._read_exact(length)
            if payload is None:
                break
            try:
                data = json.loads(payload.decode("utf-8"))
            except (UnicodeDecodeError, json.JSONDecodeError):
                continue
            with self._cond:
                self.events.append(Event(time.monotonic(), data))
                self._cond.notify_all()
        with self._cond:
            self._eof = True
            self._cond.notify_all()

    def _read_stderr(self) -> None:
        self.stderr_log.parent.mkdir(parents=True, exist_ok=True)
        with open(self.stderr_log, "w", encoding="utf-8", errors="replace") as f:
            for raw in iter(self._proc.stderr.readline, b""):
                line = raw.decode("utf-8", errors="replace").rstrip()
                f.write(_KEY_LINE.sub(r"\1 <redacted>", line) + "\n")
                f.flush()

    # -- sending ---------------------------------------------------------
    def send(self, cmd: dict) -> None:
        payload = json.dumps(cmd).encode("utf-8")
        self._proc.stdin.write(struct.pack(">I", len(payload)) + payload)
        self._proc.stdin.flush()

    # -- waiting ---------------------------------------------------------
    def mark(self) -> int:
        with self._cond:
            return len(self.events)

    def events_since(self, index: int) -> list:
        with self._cond:
            return list(self.events[index:])

    def wait_for(
        self,
        predicate: Callable[[Event], bool],
        timeout: float,
        since: int = 0,
    ) -> Optional[Event]:
        """Return the first event at or after `since` matching `predicate`."""
        deadline = time.monotonic() + timeout
        seen = since
        with self._cond:
            while True:
                while seen < len(self.events):
                    ev = self.events[seen]
                    seen += 1
                    if predicate(ev):
                        return ev
                remaining = deadline - time.monotonic()
                if remaining <= 0 or self._eof:
                    return None
                self._cond.wait(timeout=min(remaining, 0.5))

    def alive(self) -> bool:
        return self._proc is not None and self._proc.poll() is None

    def shutdown(self) -> None:
        if self._proc is None:
            return
        try:
            self.send({"cmd": "stop"})
            time.sleep(0.5)
            self.send({"cmd": "shutdown"})
        except (OSError, ValueError):
            pass
        try:
            self._proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            self._proc.kill()
        for stream in (self._proc.stdin, self._proc.stdout, self._proc.stderr):
            try:
                stream.close()
            except OSError:
                pass
