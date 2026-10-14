"""Deterministic checks for resolve_device (run: uv run python test_device.py).

Covers the S7 rule: KITTEN_DEVICE selects where every model process runs.
'auto' follows CUDA availability; explicit cuda/cpu passes through (a cuda
request on a machine without CUDA fails at model load, by design); anything
else is rejected.
"""

from server import resolve_device


def main():
    assert resolve_device(None, True) == "cuda"
    assert resolve_device(None, False) == "cpu"
    assert resolve_device("auto", False) == "cpu"
    assert resolve_device("AUTO", True) == "cuda"  # case-insensitive
    assert resolve_device("cuda", False) == "cuda"  # explicit wins
    assert resolve_device("CPU", True) == "cpu"

    try:
        resolve_device("tpu", True)
    except ValueError:
        pass
    else:
        raise AssertionError("invalid device must raise ValueError")

    print("DEVICE TESTS OK")


if __name__ == "__main__":
    main()
