"""Deterministic checks for prepare_reference (run: uv run python test_reference.py).

Covers the S6 rule: references longer than the model window are trimmed to a
temp copy; the original recording file is never modified. Short references
pass through unchanged.
"""

import os
import sys
import tempfile

import numpy as np
import soundfile as sf

from server import MAX_REFERENCE_S, prepare_reference

RATE = 24_000


def write_wav(path, seconds):
    t = np.linspace(0, seconds, int(RATE * seconds), endpoint=False)
    sf.write(path, (np.sin(2 * np.pi * 220 * t) * 0.3).astype(np.float32), RATE)


def wav_seconds(path):
    info = sf.info(path)
    return info.frames / info.samplerate


def main():
    with tempfile.TemporaryDirectory() as tmp:
        long_wav = os.path.join(tmp, "long.wav")
        write_wav(long_wav, 60.0)
        before = wav_seconds(long_wav)

        used_path, used_s, total_s = prepare_reference(long_wav)

        assert total_s == 60.0, f"total_s should report the full clip: {total_s}"
        assert used_path != long_wav, "long reference must be trimmed to a copy"
        assert abs(used_s - MAX_REFERENCE_S) < 0.1, f"used_s: {used_s}"
        assert abs(wav_seconds(used_path) - MAX_REFERENCE_S) < 0.1, "trimmed clip length"
        assert abs(wav_seconds(long_wav) - before) < 0.01, "original must stay untouched"

        short_wav = os.path.join(tmp, "short.wav")
        write_wav(short_wav, 10.0)
        used_path, used_s, total_s = prepare_reference(short_wav)
        assert used_path == short_wav, "short reference must pass through"
        assert used_s == 10.0 and total_s == 10.0, f"short: {used_s}, {total_s}"

    print("REFERENCE TESTS OK")


if __name__ == "__main__":
    sys.exit(main())
