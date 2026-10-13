# Voice Clone Desktop — Rust + Slint app with KittenTTS 2 sidecar

Desktop app (Rust + Slint) that records or imports reference audio and generates
cloned speech through a KittenTTS 2 Python sidecar (JSON-lines over stdio).

## Specs

S1. User: "ok en este proyecto quiero que probemos varios modelos, empecemos por https://huggingface.co/KittenML/kitten-tts-2" — first model under test is KittenTTS 2, integrated via the `kittenml` PyPI package in a Python sidecar.

S2. User: "ayudame a crear un proyecto con rust + slint para una aplicacion de escritorio que reciba el audio" — the deliverable is a Rust + Slint desktop app that receives audio.

S3. User answer: "Ambos" — the app records from the microphone AND imports audio files (WAV natively; other formats converted with ffmpeg when present).

S4. User answer: "Incluir KittenTTS ya" — this iteration already includes inference: a generate action clones the reference voice for the entered text and the result is playable in the app.

Environment constraints: Debian 13 (trixie), Wayland, CPU-only (8 cores, ~6 GB free RAM), ffmpeg present, no passwordless sudo. `libasound2-dev` is required for cpal (capture + playback).

## Tasks

- T1 (S2): Scaffold Rust+Slint project: Cargo.toml, build.rs, ui/app.slint, window opens. Route: inline. Commit: 6a33070.
- T2 (S2): WAV helpers (write 16-bit PCM from f32, mono downmix) — test-first. Route: inline. Commit: 6a8834a.
- T3 (S1,S4): Python sidecar server (sidecar/server.py, pyproject via uv): model loads once, ready event, generate cmd with reference or built-in voice. Route: inline. Commit: 3b10bd8.
- T4 (S4): Rust sidecar client (spawn `uv run python server.py`, id-matched JSON-lines responses, timeout) — test-first against a mock sidecar. Route: inline. Commit: 62fd42b.
- T5 (S3): Mic capture via cpal, record/stop, downmix, save WAV. Route: inline. Commit: (with app shell). [blocked: libasound2-dev]
- T6 (S3): Playback via rodio. Route: inline. Commit: (with app shell). [blocked: libasound2-dev]
- T7 (S3): File import via rfd; ffmpeg converts non-WAV to 24 kHz mono WAV. Route: inline. Commit: 6a33070.
- T8 (S2,S4): Wire UI actions (record/import/play/generate/status) to backend. Route: inline. Commit: 6a33070.
- T9 (all): cargo build + cargo test + app launch smoke. Route: inline.
- T10 (S1,S4): uv sync sidecar, kitten-tts-2 weights=emb4, generate smoke with built-in voice. Route: inline. Commit: 3b10bd8 (smoke.py).

## Log

L1. User: "cual es el mejor modelo pequeño para clonar la voz?" (research; F5-TTS, Pocket TTS, KittenTTS surfaced).
L2. User: "ok en este proyecto quiero que probemos varios modelos, empecemos por https://huggingface.co/KittenML/kitten-tts-2"
L3. User: "ayudame a crear un proyecto con rust + slint para una aplicacion de escritorio que reciba el audio"
L4. User choices: "Ambos" (mic + files), "Incluir KittenTTS ya" (sidecar this iteration).
L5. Environment: Debian 13 trixie, Wayland, CPU-only, ffmpeg present, libasound2-dev missing (no passwordless sudo) → T5/T6 blocked until user installs it.
L6. Verification evidence: cargo test RED (8 todo!() failures) → implementations → GREEN 10/10. Fix-ups during GREEN: slint-build moved to [build-dependencies]; Window has no `foreground` property; WavReader mut in tests; wait_timeout_while returns WaitTimeoutResult; WAV scale is +32768.0 (f32::from(i16::MIN) is negative — sign inversion caught by the clamp test).
L7. Sidecar smoke (T10): SMOKE OK — model kitten-tts-2 (emb4) loaded, generated 102240 samples (4.26 s) at 24 kHz with built-in voice Bruno; elapsed 50 s includes first-download + torch warmup on CPU.
L8. cpal/rodio kept behind `audio` feature (default off) so the shell builds/tests without ALSA headers; flip to default once T5/T6 verify.
L9. Launch smoke (T9): `cargo run` alive 35 s on Wayland, no panics; sidecar spawned via uv; cargo test 10/10 after warning cleanup (sample rate surfaced in generate status; test-only elements annotated). README added.
