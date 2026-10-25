"""Optional Rust analysis core (`opentrace_core`, built from `core/opentrace-py`).

The Python modules stay the reference implementation; when the extension is importable it is used for the hot
loops and produces bit-identical results (tests/test_core_parity.py). Set OPENTRACE_CORE=python to force the
reference path.
"""
import os

CORE = None
if os.environ.get('OPENTRACE_CORE', 'auto').lower() != 'python':
    try:
        import opentrace_core as CORE  # noqa: N811
    except ImportError:
        CORE = None


def available():
    return CORE is not None
