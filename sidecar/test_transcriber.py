"""Deterministic checks for transcribe_reference (run: uv run python test_transcriber.py).

Covers the S8 dispatch rule: a caller-provided reference_text always wins;
KITTEN_TRANSCRIBER=whistle transcribes with needle; =whisper returns None so
kittenml's internal Whisper handles it; unknown backends are rejected.
"""

from server import transcribe_reference


def boom(_path):
    raise AssertionError("must not transcribe")


def fake_whistle(_path, language=None):
    return f"texto de prueba ({language})"


def main():
    text, used = transcribe_reference(
        "  hola mundo  ", "/x.wav", "whistle", transcriber=boom)
    assert (text, used) == ("hola mundo", "manual"), (text, used)

    text, used = transcribe_reference(
        None, "/x.wav", "whistle", transcriber=fake_whistle)
    assert (text, used) == ("texto de prueba (None)", "whistle"), (text, used)

    # S10: the UI-selected language reaches the whistle transcriber.
    seen = {}

    def capturing_whistle(_path, language=None):
        seen["language"] = language
        return "texto de prueba"

    text, used = transcribe_reference(
        None, "/x.wav", "whistle", transcriber=capturing_whistle, language="es")
    assert (text, used) == ("texto de prueba", "whistle"), (text, used)
    assert seen["language"] == "es", seen

    transcribe_reference(None, "/x.wav", "whistle", transcriber=capturing_whistle)
    assert seen["language"] is None, seen

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
