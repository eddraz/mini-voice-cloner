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
`{"id":N,"cmd":"generate","text":...,"reference":...,"out":...}` lines and the
sidecar emits a `ready` event, then id-matched `ok/error` responses.

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

The crate is Windows-ready: `rfd` uses the native file dialog there and
cpal/rodio use WASAPI (no ALSA). Not yet compiled on Windows — `ffmpeg` is
only needed for importing compressed audio.

## Status

- WAV helpers, sidecar client, import: unit-tested (RED→GREEN).
- Reading-script pane + 15-min recording cap: implemented; elapsed + cap
  logic covered by `format_elapsed` test; sidecar reference windowing
  covered by `sidecar/test_reference.py` (REFERENCE TESTS OK).
- Sidecar smoke: generated 24 kHz clip with a built-in voice on CPU.
- Mic capture + playback: verified — ALSA headers installed, `audio` default;
  cpal 0.18 / rodio 0.23 API fixes applied, app launches with the default mic
  detected. Interactive record/clone test pending a real reading.
