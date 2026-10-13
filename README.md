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
cargo run                       # app shell; audio feature off until ALSA headers exist
cargo run --features audio      # full app: mic capture + playback
cargo test                      # unit tests (no model needed; mock sidecar)
```

First run downloads the KittenTTS 2 weights (~506 MiB, `weights="emb4"`) into
the HuggingFace cache and takes a while; later starts load from disk.

## Status

- WAV helpers, sidecar client, import: unit-tested (RED→GREEN).
- Sidecar smoke: generated 24 kHz clip with a built-in voice on CPU.
- Mic capture + playback: code ready behind `--features audio`, pending ALSA
  headers on the dev machine.
