"""KittenTTS 2 sidecar: JSON-lines server over stdio.

The desktop app spawns this process and speaks the protocol documented in
src/tts/sidecar.rs. The model is loaded once in a background thread at
startup; requests received before the model is ready fail fast.

Messages (one JSON object per line):
- emit {"event": "ready"} once the model is loaded, or
  {"event": "load_error", "error": "..."} on startup failure.
- {"id":N,"cmd":"status"} -> {"id":N,"ok":true,"ready":bool,"error":str|null}
- {"id":N,"cmd":"generate","text":"...","reference":"/path.wav","out":"/out.wav"}
    (or "voice":"Bruno" instead of "reference")
  -> {"id":N,"ok":true,"elapsed_s":1.23,"sample_rate":24000,"samples":N}
  or {"id":N,"ok":false,"error":"..."}
"""

import json
import os
import sys
import threading
import time

emit_lock = threading.Lock()
state = {"model": None, "error": None}


def emit(obj):
    with emit_lock:
        print(json.dumps(obj), flush=True)


def load_model():
    try:
        from kittenml import KittenTTS

        weights = os.environ.get("KITTEN_WEIGHTS", "emb4")
        state["model"] = KittenTTS("KittenML/kitten-tts-2", weights=weights)
        emit({"event": "ready"})
    except Exception as exc:  # noqa: BLE001 - surfaced to the host verbatim
        state["error"] = str(exc)
        emit({"event": "load_error", "error": str(exc)})


def handle(req):
    rid = req.get("id")
    cmd = req.get("cmd")
    try:
        if cmd == "status":
            emit({
                "id": rid,
                "ok": True,
                "ready": state["model"] is not None,
                "error": state["error"],
            })
        elif cmd == "generate":
            model = state["model"]
            if model is None:
                raise RuntimeError(f"model not loaded: {state['error']}")
            text = req["text"]
            out = req["out"]
            reference = req.get("reference")
            voice = req.get("voice", "Bruno")
            start = time.time()
            if reference:
                audio = model.generate(text, reference=reference)
            else:
                audio = model.generate(text, voice=voice)
            import soundfile as sf

            sf.write(out, audio, model.sample_rate)
            emit({
                "id": rid,
                "ok": True,
                "elapsed_s": round(time.time() - start, 2),
                "sample_rate": model.sample_rate,
                "samples": int(len(audio)),
            })
        else:
            emit({"id": rid, "ok": False, "error": f"unknown cmd {cmd!r}"})
    except Exception as exc:  # noqa: BLE001 - surfaced to the host verbatim
        emit({"id": rid, "ok": False, "error": str(exc)})


def main():
    threading.Thread(target=load_model, daemon=True).start()
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except json.JSONDecodeError as exc:
            emit({"id": None, "ok": False, "error": f"bad json: {exc}"})
            continue
        handle(req)


if __name__ == "__main__":
    main()
