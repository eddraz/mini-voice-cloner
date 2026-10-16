"""End-to-end check against a real reference recording (headless, no GUI).

Usage: uv run python e2e_reference.py <reference.wav> <out.wav> "<text>"

Spawns server.py, waits for the model, sends one generate request with the
reference, and verifies the output file. Exercises absolute-path handling,
the 30 s reference window, Whisper transcription and CPU inference.
"""

import json
import os
import subprocess
import sys
import threading
import time

if len(sys.argv) != 4:
    print("usage: e2e_reference.py <reference.wav> <out.wav> <text>")
    sys.exit(2)

REFERENCE, OUT, TEXT = sys.argv[1], sys.argv[2], sys.argv[3]
DEADLINE = time.time() + 60 * 12


def fail(message):
    print(f"E2E FAIL: {message}", flush=True)
    sys.exit(1)


proc = subprocess.Popen(
    [sys.executable, "server.py"],
    env={**os.environ, "PYTHONUTF8": "1"},  # Windows: kittenml reads UTF-8 JSON
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

ready = False
while time.time() < DEADLINE and not reader_done.is_set():
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
    fail("model never became ready")

proc.stdin.write(json.dumps({
    "id": 1,
    "cmd": "generate",
    "text": TEXT,
    "reference": REFERENCE,
    "out": OUT,
}) + "\n")
proc.stdin.flush()

response = None
while time.time() < DEADLINE and not response and not reader_done.is_set():
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

if not os.path.exists(OUT) or os.path.getsize(OUT) == 0:
    fail(f"output file missing or empty: {OUT}")

print(
    f"E2E OK: {OUT} ({os.path.getsize(OUT)} bytes) "
    f"reference_used_s={response.get('reference_used_s')} "
    f"reference_total_s={response.get('reference_total_s')} "
    f"elapsed_s={response.get('elapsed_s')}",
    flush=True,
)
proc.kill()
