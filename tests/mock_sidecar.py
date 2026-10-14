"""Mock sidecar used by the Rust client tests: speaks the sidecar protocol
without loading any model.

- Emits {"event": "ready"} at startup.
- Handles each request in a thread: generate returns ok (or ok:false when
  text == "fail" with error "boom"); every other cmd returns ok.
- With --stall, requests never get a response (timeout testing).
"""

import json
import sys
import threading
import time

stall = "--stall" in sys.argv
lock = threading.Lock()


def emit(obj):
    with lock:
        print(json.dumps(obj), flush=True)


emit({"event": "ready", "device": "cuda:0"})

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        req = json.loads(line)
    except json.JSONDecodeError:
        continue
    if stall:
        continue

    def work(req):
        time.sleep(0.05)
        if req.get("cmd") == "generate":
            if req.get("text") == "fail":
                emit({"id": req.get("id"), "ok": False, "error": "boom"})
            else:
                emit({
                    "id": req.get("id"),
                    "ok": True,
                    "elapsed_s": 0.05,
                    "sample_rate": 24000,
                    "samples": 1200,
                })
        else:
            emit({"id": req.get("id"), "ok": True})

    threading.Thread(target=work, args=(req,), daemon=True).start()
