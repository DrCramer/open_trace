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


@unittest.skipUnless(_core.available() and hasattr(_core.CORE, 'best_paths'), 'opentrace_core.best_paths not available')
class RatioSizingParityTests(unittest.TestCase):
    """ratio_sizing._best_paths: Rust and Python must agree on scores (bit for bit), paths and transition counts."""

    def check(self, scans, heights, bp, weight):
        from genemapper_open import ratio_sizing
        py = ratio_sizing._best_paths_py(scans, heights, bp, weight)
        rs = ratio_sizing._best_paths(scans, heights, bp, weight)
        self.assertEqual(py[1], rs[1], 'transitions')
        self.assertEqual(len(py[0]), len(rs[0]))
        for (ps, pp), (rsc, rp) in zip(py[0], rs[0]):
            self.assertEqual(pp, rp)
            self.assertEqual(ps.hex(), rsc.hex())

    def test_random_inputs(self):
        rng = random.Random(7)
        for _ in range(60):
            n = rng.randint(3, 26)
            count = n + rng.randint(0, 14)
            bp = sorted(rng.sample(range(20, 600), n))
            scans = sorted(rng.sample(range(500, 9000), count))
            heights = [rng.choice([rng.uniform(100, 9000), 1000.0]) for _ in range(count)]
            self.check(scans, heights, bp, rng.choice([0.0, 0.3, 0.8, 0.95, 1.0]))

    def test_ties_and_even_spacing(self):
        for count, n in ((10, 5), (14, 8), (20, 20)):
            scans = [1000.0 + 37.0 * i for i in range(count)]
            self.check(scans, [500.0] * count, [60.0 + 20.0 * i for i in range(n)], 0.8)
            self.check(scans, [500.0] * count, [60.0 + 20.0 * i for i in range(n)], 1.0)

    def test_two_peaks_more_than_markers(self):
        self.check([100.0, 150.0, 210.0, 300.0], [10.0, 20.0, 15.0, 30.0], [50.0, 90.0, 140.0], 0.5)
