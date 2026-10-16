"""Smoke test for the sidecar: spawn server.py, wait for the model-ready
event, generate one clip with a built-in voice, and verify the WAV lands.

Run with: uv run python smoke.py
"""

import json
import os
import subprocess
import sys
import threading
import time

TEXT = "[joyful] Hola, esta es una prueba de síntesis de voz con KittenTTS. <laugh>"
OUT = "../recordings/smoke-out.wav"


def fail(message):
    print(f"SMOKE FAIL: {message}", flush=True)
    sys.exit(1)


proc = subprocess.Popen(
    [sys.executable, "server.py"],
    # PYTHONUTF8=1: kittenml opens its voice metadata JSON without an
    # encoding and Windows' cp1252 default breaks the model load.
    env={**os.environ, "PYTHONUTF8": "1"},
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    text=True,
    bufsize=1,
)
lines = []
reader_done = threading.Event()


def read_stdout():
    for line in proc.stdout:
        lines.append(line.strip())
    reader_done.set()


threading.Thread(target=read_stdout, daemon=True).start()

deadline = time.time() + 60 * 20  # first run downloads the weights
ready = False
while time.time() < deadline and not reader_done.is_set():
    pending, lines[:] = lines[:], []
    for line in pending:
        print(f"sidecar: {line}", flush=True)
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get("event") == "ready":
            ready = True
        elif msg.get("event") == "load_error":
            fail(f"model load failed: {msg.get('error')}")
    if ready:
        break
    time.sleep(0.5)

if not ready:
    fail(f"model never became ready (alive={proc.poll() is None})")

proc.stdin.write(json.dumps({
    "id": 1,
    "cmd": "generate",
    "text": TEXT,
    "voice": "Bruno",
    "preset": "expressive",
    "out": OUT,
}) + "\n")
proc.stdin.flush()

response = None
while time.time() < deadline and not response and not reader_done.is_set():
    pending, lines[:] = lines[:], []
    for line in pending:
        print(f"sidecar: {line}", flush=True)
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get("id") == 1:
            response = msg
    time.sleep(0.2)

if response is None:
    fail("no response to generate request")
if not response.get("ok"):
    fail(f"generate failed: {response.get('error')}")

import os

if not os.path.exists(OUT) or os.path.getsize(OUT) == 0:
    fail(f"output file missing or empty: {OUT}")

print(
    f"SMOKE OK: {OUT} ({os.path.getsize(OUT)} bytes, "
    f"{response.get('elapsed_s')}s, rate={response.get('sample_rate')})",
    flush=True,
)
proc.kill()
