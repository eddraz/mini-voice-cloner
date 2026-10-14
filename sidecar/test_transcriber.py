"""Deterministic checks for transcribe_reference (run: uv run python test_transcriber.py).

Covers the S8 dispatch rule: a caller-provided reference_text always wins;
KITTEN_TRANSCRIBER=whistle transcribes with needle; =whisper returns None so
kittenml's internal Whisper handles it; unknown backends are rejected.
"""

from server import transcribe_reference


def boom(_path):
    raise AssertionError("must not transcribe")


def fake_whistle(_path):
    return "texto de prueba"


def main():
    text, used = transcribe_reference(
        "  hola mundo  ", "/x.wav", "whistle", transcriber=boom)
    assert (text, used) == ("hola mundo", "manual"), (text, used)

    text, used = transcribe_reference(
        None, "/x.wav", "whistle", transcriber=fake_whistle)
    assert (text, used) == ("texto de prueba", "whistle"), (text, used)

    text, used = transcribe_reference(
        None, "/x.wav", "whisper", transcriber=boom)
    assert text is None and used == "whisper", (text, used)

    try:
        transcribe_reference(None, "/x.wav", "tpu", transcriber=boom)
    except ValueError:
        pass
    else:
        raise AssertionError("unknown backend must raise ValueError")

    print("TRANSCRIBER TESTS OK")


if __name__ == "__main__":
    main()
