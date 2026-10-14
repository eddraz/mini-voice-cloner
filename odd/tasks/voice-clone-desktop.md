# Voice Clone Desktop — Rust + Slint app with KittenTTS 2 sidecar

Desktop app (Rust + Slint) that records or imports reference audio and generates
cloned speech through a KittenTTS 2 Python sidecar (JSON-lines over stdio).

## Specs

S1. User: "ok en este proyecto quiero que probemos varios modelos, empecemos por https://huggingface.co/KittenML/kitten-tts-2" — first model under test is KittenTTS 2, integrated via the `kittenml` PyPI package in a Python sidecar.

S2. User: "ayudame a crear un proyecto con rust + slint para una aplicacion de escritorio que reciba el audio" — the deliverable is a Rust + Slint desktop app that receives audio.

S3. User answer: "Ambos" — the app records from the microphone AND imports audio files (WAV natively; other formats converted with ffmpeg when present).

S4. User answer: "Incluir KittenTTS ya" — this iteration already includes inference: a generate action clones the reference voice for the entered text and the result is playable in the app.

S5. User: "agrega un texto de prueba para poder leer al grabar la voz" — the recording view shows a reading script to read aloud while recording.

S6. User: "el audio deberia poder durar 15 mins para que funcione correctamente" — recordings can last up to 15 minutes (auto-stop at 15:00; the full WAV is kept on disk). Evidence: kittenml `build_reference_prefix` embeds all reference codec tokens into the prompt with no truncation (~75 tokens/s → a 15-min clip would break generation), so only a bounded window (30 s, KittenTTS's documented maximum) is passed to the model while the full recording stays on disk.

S7. User: "puedes en la GPU agregar todos los procesos que sean posible para maquinas que lo soporten?" — the sidecar must place every GPU-capable process on the GPU when the machine supports it: explicit device selection (KITTEN_DEVICE=auto|cuda|cpu, auto default = kittenml's own cuda-if-available), propagated to KittenTTS so the LM, codec, speaker encoder and Whisper transcriber all share one device, plus a ready event carrying the resolved device (and VRAM when CUDA) so the UI shows where inference actually runs.

S8. User: "en lugar de whisper podemos usar https://huggingface.co/Cactus-Compute/whistle" — reference transcription defaults to whistle (needle engine, Apache 2.0, 16.9 MB, Spanish supported, ~4 s per 30 s clip on CPU); `KITTEN_TRANSCRIBER=whisper` restores kittenml's internal Whisper large-v3; a `reference_text` in the request always wins regardless of backend.

S9. User: "deberia poder pausar la reproduccion no que se duplique el audio" — playback is a single persistent output channel: clicking ▶ starts the clip, clicking again pauses it, again resumes it; playing another clip stops the current one; starting a recording stops playback. No overlapping audio, ever.

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
- T11 (S5): Reading-script pane in the recording view (TextEdit, default Spanish passage, editable for custom scripts). Route: inline. Commit: feat(ui).
- T12 (S6): 15-minute recording support: m:ss elapsed (test-first) + auto-stop cap at 15:00. Route: inline. Commit: feat(ui).
- T13 (S6): Sidecar reference windowing: `prepare_reference` trims to 30 s before model.generate (full WAV untouched); test-first. Route: inline. Commit: feat(sidecar).
- T14 (all): cargo test + launch smoke + docs/memory. Route: inline.
- T15 (S7): sidecar device control: `resolve_device` (test-first) + load_model wiring (device → KittenTTS, ready event with device/cuda/vram_gb). Route: inline. Commit: feat(sidecar).
- T16 (S7): Rust client surfaces the device: parse `device` from the ready event, `device_label()`, status shows "model ready (cuda|cpu)" — test-first via mock. Route: inline. Commit: feat(tts)+feat(app).
- T17 (S7): launch smoke showing "model ready (cpu)" on this machine (chain proof); README GPU section; Log L16. Route: inline.
- T18 (S8): cactus-needle dependency + `transcribe_reference` dispatch (test-first): request reference_text wins; whistle path (soxr 16 kHz resample + needle); whisper path delegates to kittenml. Route: inline. Commit: feat(sidecar).
- T19 (S8): E2E generate through the whistle path with the user's real reference; README transcription section; Log L18. Route: inline.
- T20 (S9): `next_action` click-decision function (test-first) + `PlaybackController` (persistent sink, single Player, pause/resume/stop, poll_finished); replace the blocking per-click `play()`. Route: inline. Commit: fix(audio).
- T21 (S9): UI wiring — ▶/⏸ per source (reference/result), flags refreshed after every action and on natural finish; recording stops playback; launch smoke + Log L19. Route: inline. Commit: feat(app)+docs.

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
L10. User: "agrega un texto de prueba para poder leer al grabar la voz, el audio deberia poder durar 15 mins para que funcione correctamente" → S5, S6 added; T11-T14 opened. Evidence for the 30 s model window: prompt.py build_reference_prefix (no truncation, all codec tokens in prompt); KittenTTS README ("Give it 5-30 seconds").
L11. Verification: format_elapsed RED→GREEN (cargo test 11/11 incl. TextEdit UI compile); sidecar/test_reference.py RED (ImportError) → REFERENCE TESTS OK (60 s clip trimmed to 30 s temp copy, original untouched; 10 s clip passes through).
L12. User-reported generate failure (screenshot): "Error opening 'recordings/audio-24k.wav': System error." Root cause: the sidecar runs with CWD sidecar/ but the client sent repo-relative paths. Fix 71aff6b: build_generate_request absolutizes reference/out (cargo test 12/12). Headless E2E with the user's real 17.5-min 24 kHz reference: E2E OK — reference_used_s=30.0 of 1048.4, 5.4 s audio generated at 24 kHz; elapsed 386 s dominated by Whisper transcription of the reference + CPU inference (kittenml caches the reference, so repeat generates in the same sidecar session skip re-transcription).
L13. T5/T6 activation: user installed libasound2-dev 1.2.14; `audio` flipped to default feature. First compile of capture.rs/playback.rs surfaced 6 API errors — fixed against real sources: cpal 0.18 (config by value, FnMut(cpal::Error) instead of StreamError, SampleRate is a u32 alias, dasp `from_sample_` with trailing underscore) and rodio 0.23 (OutputStream/Sink removed → DeviceSinkBuilder::open_default_sink() + play(mixer, src) + Player::sleep_until_end). cargo test 12/12 with audio; launch smoke 30 s with default mic detected.
L14. Windows portability: rfd moved to per-platform deps (unix keeps xdg-portal, windows gets the native backend). First attempt put the [target.*] section headers mid-[dependencies], which silently swallowed the deps that followed (13 compile errors) — restructured to the file end. cargo test 12/12 on Linux; Windows compile still untested.
L15. User: "puedes en la GPU agregar todos los procesos que sean posible para maquinas que lo soporten?" → S7, T15-T17. Verified against installed kittenml: KittenTTS.__init__ propagates `device` to LM, S3Codec, SpeakerEncoder AND ReferenceTranscriber (my earlier "transcriber lacks device" was a misread of a truncated sed window) — so explicit device control + telemetry is the whole task. torch in venv is 2.14.1+cu130.
L16. Verification: resolve_device RED (ImportError) → DEVICE TESTS OK (auto follows CUDA, explicit passes through, invalid rejected); Rust ready_event_reports_device RED (todo panic) → cargo test 13/13; real sidecar ready event: {"device":"cpu","cuda":false,"vram_gb":null} on this machine.
L17. User: "en lugar de whisper podemos usar https://huggingface.co/Cactus-Compute/whistle" → S8, T18-T19. Probe on the user's real audio (audio-24k.wav): Apache 2.0; needle imports on Linux x86_64; whistle transcribes coherent Spanish at 3.3-4.6 s per 30 s clip (13.9 s first call incl. engine load) vs ~300 s for whisper-large-v3 on this CPU; proper nouns mangled but text usable. Also on record: current Whisper path forces language="en" on Spanish audio.
L18. Verification: transcribe_reference RED (ImportError) → TRANSCRIBER TESTS OK (manual wins, whistle dispatches, whisper delegates, unknown rejected) + DEVICE/REFERENCE regressions OK. Headless E2E through the whistle path: E2E OK — transcript_backend=whistle, reference_used_s=30.0 of 1048.4, 5.8 s audio at 24 kHz, elapsed 169 s vs 386 s on the whisper path (2.3× faster, cold start). README transcription section added; KITTEN_TRANSCRIBER=whisper reverts.
L19. User (screenshot + report): "deberia poder pausar la reproduccion no que se duplique el audio" — each ▶ click spawned an independent output sink, so repeated clicks overlapped audio with no pause. → S9, T20-T21.
