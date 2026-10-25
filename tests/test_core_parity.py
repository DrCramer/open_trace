"""The optional Rust core (opentrace_core) must give results identical to the Python reference."""
import json
import math
import random
import unittest

from genemapper_open import _core, preprocessing, sg_peaks


def synthetic(rng, n=4000):
    base = [200 + int(30 * math.sin(i / 300)) for i in range(n)]
    for c in range(80, n - 80, rng.randint(40, 120)):
        h = rng.uniform(50, 30000)
        w = rng.uniform(1.5, 6)
        for t in range(c - 25, c + 26):
            base[t] += int(h * math.exp(-((t - c) / w) ** 2))
    for i in rng.sample(range(n), 20):
        base[i] += rng.randint(500, 5000)  # spikes
    return [v + rng.randint(-15, 15) for v in base]


@unittest.skipUnless(_core.available(), 'opentrace_core extension not installed')
class CoreParityTests(unittest.TestCase):
    def both(self, fn):
        rust = _core.CORE
        try:
            _core.CORE = None
            ref = fn()
        finally:
            _core.CORE = rust
        return ref, fn()

    def test_preprocessing_identical(self):
        rng = random.Random(11)
        for _ in range(6):
            x = synthetic(rng, rng.randint(300, 6000))
            for kw in ({'window': 101, 'spike_window': 5, 'smoothing_window': 1, 'smooth_passes': 0},
                       {'window': 251, 'spike_window': 9, 'smoothing_window': 21, 'smooth_passes': 2},
                       {'window': 3, 'spike_window': 1, 'smoothing_window': 1, 'smooth_passes': 1}):
                ref, fast = self.both(lambda: preprocessing.prepare_trace(x, **kw))
                self.assertEqual(json.dumps(ref), json.dumps(fast))

    def test_sg_peaks_identical(self):
        rng = random.Random(12)
        for _ in range(8):
            x = synthetic(rng, rng.randint(500, 6000))
            sig = preprocessing.prepare_trace(x, window=101, spike_window=5)['signal']
            for kw in ({}, {'window': 9, 'threshold': 50.0, 'min_width': 1.0},
                       {'window': 21, 'tau_begin': 3.0, 'tau_end': -2.0, 'offscale_intervals': [(100, 140), (900, 901)]},
                       {'window': 15, 'degree': 5}):
                ref, fast = self.both(lambda: sg_peaks.detect_sg_peaks(sig, **kw))
                self.assertEqual(json.dumps(ref), json.dumps(fast))
                self.assertGreater(ref['diagnostics']['sg_candidates'], 0)

    def test_validation_identical(self):
        for bad in ([1, 2.0], [True], [2 ** 31], [-(2 ** 31) - 1], (1, 'a')):
            for fn in (lambda b: preprocessing.prepare_trace(b, window=3, spike_window=1),
                       lambda b: sg_peaks.cubic_sg_derivative(list(b) * 10 if isinstance(b, list) else b)):
                errs = []
                for core in (None, _core.CORE):
                    saved = _core.CORE
                    _core.CORE = core
                    try:
                        fn(bad)
                        errs.append(None)
                    except (ValueError, TypeError) as e:
                        errs.append(type(e))
                    finally:
                        _core.CORE = saved
                self.assertEqual(errs[0], errs[1], bad)


if __name__ == '__main__':
    unittest.main()
