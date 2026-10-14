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
import tempfile
import threading
import time

emit_lock = threading.Lock()
state = {"model": None, "error": None}

# KittenTTS embeds the whole reference clip as prompt tokens (see
# build_reference_prefix in prompt.py) with no internal truncation, so long
# recordings are trimmed to this window before reaching the model. The full
# recording always stays on disk; override with KITTEN_REFERENCE_MAX_S.
MAX_REFERENCE_S = float(os.environ.get("KITTEN_REFERENCE_MAX_S", "30"))


def emit(obj):
    with emit_lock:
        print(json.dumps(obj), flush=True)


def prepare_reference(path, max_s=MAX_REFERENCE_S):
    """Return (path_to_use, used_s, total_s) for a reference recording.

    Clips longer than the model window (max_s) are trimmed to their first
    max_s seconds into a temp WAV copy; the original file is never modified.
    """
    import soundfile as sf

    info = sf.info(path)
    total_s = info.frames / info.samplerate
    if total_s <= max_s:
        return path, total_s, total_s
    frames = int(max_s * info.samplerate)
    data, sr = sf.read(path, frames=frames)
    tmp = tempfile.NamedTemporaryFile(suffix=".wav", delete=False)
    tmp.close()
    sf.write(tmp.name, data, sr)
    return tmp.name, frames / sr, total_s


def resolve_device(setting, cuda_available):
    """Map KITTEN_DEVICE (auto|cuda|cpu, case-insensitive) to the device
    argument for KittenTTS.

    'auto' resolves to cuda when CUDA is available, cpu otherwise. Explicit
    values pass through: a cuda request on a machine without CUDA fails at
    model load with a clear error, by design.
    """
    value = (setting or "auto").strip().lower()
    if value == "auto":
        return "cuda" if cuda_available else "cpu"
    if value in ("cuda", "cpu"):
        return value
    raise ValueError(f"invalid KITTEN_DEVICE {setting!r}: use auto, cuda or cpu")


def load_model():
    try:
        import torch
        from kittenml import KittenTTS

        weights = os.environ.get("KITTEN_WEIGHTS", "emb4")
        device = resolve_device(
            os.environ.get("KITTEN_DEVICE"), torch.cuda.is_available())
        state["model"] = KittenTTS(
            "KittenML/kitten-tts-2", weights=weights, device=device)
        vram_gb = None
        if device == "cuda":
            vram_gb = round(torch.cuda.memory_allocated() / 1e9, 2)
        emit({
            "event": "ready",
            "device": str(getattr(state["model"], "device", device)),
            "cuda": device == "cuda",
            "vram_gb": vram_gb,
        })
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
            trimmed_temp = None
            ref_used_s = None
            ref_total_s = None
            try:
                if reference:
                    ref_path, ref_used_s, ref_total_s = prepare_reference(reference)
                    if ref_path != reference:
                        trimmed_temp = ref_path
                    audio = model.generate(text, reference=ref_path)
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
                    "reference_used_s": round(ref_used_s, 1) if ref_used_s else None,
                    "reference_total_s": round(ref_total_s, 1) if ref_total_s else None,
                })
            finally:
                if trimmed_temp:
                    try:
                        os.unlink(trimmed_temp)
                    except OSError:
                        pass
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
