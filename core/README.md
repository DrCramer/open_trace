# OpenTrace core (Rust)

Analysis core shared by the server (Python extension via PyO3), the web client (WebAssembly) and the Windows client
(Tauri). The Python modules in `genemapper_open/` remain the reference: every ported function must give bit-identical
results (`tests/test_core_parity.py`, corpus check over all available .fsa).

Crates:
- `opentrace-core` — pure Rust, no dependencies: `preprocessing` (flat morphology baseline, correction, smoothing),
  `sg` (cubic Savitzky-Golay derivative, peak bounds/heights/widths/areas), `fsum` (CPython `math.fsum` port),
  `numeric` (Python int/int true division, floor division).
- `opentrace-py` — Python extension `opentrace_core`; `genemapper_open/_core.py` picks it up automatically
  (`OPENTRACE_CORE=python` forces the reference path).

Build the extension:

    pip install maturin
    cd core/opentrace-py && maturin build --release -o dist && pip install dist/opentrace_core-*.whl

Tests: `cargo test -p opentrace-core`; `python -m unittest tests.test_core_parity`.
