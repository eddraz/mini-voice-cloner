# Voice Clone Lab

Desktop app (Rust + Slint) that receives reference audio — recorded from the
microphone or imported from a file — and generates cloned speech with
[KittenTTS 2](https://huggingface.co/KittenML/kitten-tts-2) through a Python
sidecar (JSON-lines over stdio).

## Requirements

- Rust (1.98+), [`uv`](https://docs.astral.sh/uv/) (for the Python sidecar)
- `ffmpeg` (imports compressed audio formats)
- Linux desktop (Wayland/X11). For microphone capture and playback the ALSA
  dev headers are required:

  ```bash
  sudo apt install libasound2-dev pkg-config
  ```

## Architecture

```
ui/app.slint            Slint UI (record / import / generate / playback)
src/main.rs             UI wiring + 250 ms status ticker
src/audio/wav.rs        16-bit PCM WAV writer + mono downmix (tested)
src/audio/import.rs     ffmpeg conversion to 24 kHz mono WAV (tested)
src/audio/capture.rs    cpal mic capture        [feature = "audio"]
src/audio/playback.rs   rodio playback          [feature = "audio"]
src/tts/sidecar.rs      JSON-lines sidecar client (tested vs mock)
sidecar/server.py       kittenml KittenTTS 2 server (model loads once)
sidecar/smoke.py        end-to-end smoke test (built-in voice)
tests/mock_sidecar.py   protocol mock for the client tests
```

The sidecar protocol is documented in `src/tts/sidecar.rs`: the host sends
`{"id":N,"cmd":"generate","text":...,"reference":...,"out":...}` lines
(plus an optional `"preset":"stable"|"expressive"`) and the sidecar emits
a `ready` event, then id-matched `ok/error` responses.

## Build and run

```bash
uv sync --directory sidecar     # once: installs kittenml + torch (~2 GB)
cargo run                       # full app: mic capture + playback (audio is a default feature)
cargo test                      # unit tests (no model needed; mock sidecar)
```

Headless environments can build with `cargo run --no-default-features` (record
and play buttons will report the missing feature instead).

First run downloads the KittenTTS 2 weights (~506 MiB, `weights="emb4"`) into
the HuggingFace cache and takes a while; later starts load from disk.

## Recording workflow

1. Read the on-screen script aloud while recording (the pane is editable —
   paste your own text for longer sessions). Recordings auto-stop at 15:00
   (`m:ss` counter) and are saved to `recordings/ref-<timestamp>.wav`.
2. Long recordings are safe to generate from: KittenTTS embeds the whole
   reference as prompt tokens with no internal truncation (see
   `build_reference_prefix`), so the sidecar passes only the first 30 s to
   the model (`KITTEN_REFERENCE_MAX_S` overrides the window). The full
   recording file is never modified.

## Windows

The crate is Windows-ready and **verified**: compiled, `cargo test` 18/18,
sidecar smoke and a whistle-path E2E all pass on Windows 11 (Rust 1.99,
Python 3.13 via `uv`, CPU inference). Windows specifics:

- `rfd` uses the native file dialog; cpal/rodio use WASAPI (no ALSA needed).
- Pin the sidecar Python to 3.13 (`sidecar/.python-version` is committed):
  uv otherwise picks 3.15, for which `onnxruntime` (needle) has no wheel.
- PyPI torch wheels for Windows are CPU-only, so inference runs on CPU
  (`model ready (cpu)`); the CUDA notes above apply to Linux.
- The Rust spawner sets `PYTHONUTF8=1` for the sidecar: kittenml reads its
  voice-metadata JSON without an explicit encoding and the Windows cp1252
  default breaks the model load (`UnicodeDecodeError` in `_load_voices`).
- `server.py` pre-imports torch/kittenml/needle/etc. on the main thread
  before starting the load thread: importing extension DLLs from a
  secondary thread while the main thread blocks reading stdin deadlocks
  inside importlib on CPython 3.13/Windows.
- `cargo test` spawns the mock as `python3`; Windows has no such executable
  by default — `uv sync` creates the venv, and copying
  `sidecar/.venv/Scripts/python.exe` to `python3.exe` (with that dir on
  PATH) satisfies it.
- `ffmpeg` is only needed for importing compressed audio
  (`winget install Gyan.FFmpeg`).

## Expressiveness (presets + tags)

Two controls make cloned speech sound less robotic:

- **Style selector** — every generate request carries a decoding preset.
  The UI defaults to `expressive` (temperature/top_p 0.9 — livelier prosody);
  `stable` is the model's own default when the field is omitted. Long text is
  already chunked into sentences internally by kittenml, so no manual
  splitting is needed.
- **Expression tags in the text** — kitten-tts-2 reads markup verbatim:
  a leading emotion tag (`[joyful]`, `[sad]`, `[excited]`, … — 10 total),
  inline vocal events (`<laugh>`, `<sigh>`, `<pause>`, `<gasp>`, …) and
  `(((emphasis)))` spans around words. They work in the app's
  *Text to speak* field as-is, and pair best with the expressive preset.

Note: KittenTTS 2 has **no `speed` parameter** — that knob belongs to the
legacy KittenTTS 1 model (`kittentts_legacy`); in KittenTTS 2 speed control
is an unimplemented TODO in the vocoder, so pace is guided only by the
reference clip and the tags above.

## Transcription (reference transcript)

To clone, the model needs the reference transcript. `KITTEN_TRANSCRIBER`
picks who produces it:

- `whistle` (default): the 16.9 MB Cactus whistle model via the needle
  engine — Apache 2.0, Spanish supported, ~4 s per 30 s clip on CPU
  (first call also loads the engine). Better than the old default for
  Spanish references, which kittenml's Whisper forced to English.
- `whisper`: kittenml's internal Whisper large-v3 (transformers, slower on
  CPU, best accuracy, English-forced).

A `reference_text` in a generate request always wins over both — when the
UI grows a transcript field, that path is already wired.

The UI's **Reference language** select covers the languages both whistle
and kitten-tts-2 support: auto-detect, English, German, French, Spanish,
Italian, Dutch (whistle's Polish is not in kitten-tts-2's language list).
The choice travels with each generate request; `Auto-detect` lets whistle
detect the language.

## GPU

The sidecar picks the inference device with `KITTEN_DEVICE` (default `auto`:
CUDA when available, else CPU). On an NVIDIA machine every heavy process —
the 1.7 B speech LM, the S3 codec/vocoder, the speaker encoder and Whisper —
runs on CUDA automatically (kittenml propagates one device to all of them),
and the header shows `model ready (cuda:0)` plus VRAM in the ready event.
Force a value with `KITTEN_DEVICE=cuda` or `KITTEN_DEVICE=cpu`; an explicit
cuda request on a machine without CUDA fails at model load with a clear
error. The bundled torch is a CUDA 13 build, so no reinstall is needed.

## Status

- WAV helpers, sidecar client, import: unit-tested (RED→GREEN).
- Reading-script pane + 15-min recording cap: implemented; elapsed + cap
  logic covered by `format_elapsed` test; sidecar reference windowing
  covered by `sidecar/test_reference.py` (REFERENCE TESTS OK).
- Sidecar smoke: generated 24 kHz clip with a built-in voice on CPU.
- Playback: single persistent output channel (S9) — ▶ starts, ▶ again pauses
  (⏸ shown), again resumes; switching clips or starting a recording stops the
  current audio. Click-decision logic unit-tested; no overlapping playback.
- Mic capture + playback: verified — ALSA headers installed, `audio` default;
  cpal 0.18 / rodio 0.23 API fixes applied, app launches with the default mic
  detected. Interactive record/clone test pending a real reading.
