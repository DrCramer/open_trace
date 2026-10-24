"""Size-standard normalization (SFN), SG polynomial degree for the peak detector."""
import unittest

from genemapper_open.server import sample_file_normalization, _measure_morphology_channel, DETECTOR_DEFAULTS
from genemapper_open.sg_peaks import sg_derivative_weights, cubic_sg_derivative


class FakeSample:
    def __init__(self, tags):
        self.tags = tags

    def value(self, name, number=1):
        return self.tags.get((name, number))


class NormalizationTests(unittest.TestCase):
    def test_sample_file_normalization_status(self):
        # GeneMapper reads NrmS only for 3500/3200 data (DataModule: setHasNormalizationFactor 3 -> 2 -> 1)
        self.assertEqual(sample_file_normalization(FakeSample({('HCFG', 3): '3130xl', ('NrmS', 1): 1.3})),
                         {'status': 'N/A', 'factor': None})
        self.assertEqual(sample_file_normalization(FakeSample({('HCFG', 3): '3500xl'})), {'status': 'No', 'factor': None})
        self.assertEqual(sample_file_normalization(FakeSample({('HCFG', 3): '3500', ('NrmS', 1): 1.25})),
                         {'status': 'Yes', 'factor': 1.25})

    def test_factor_scales_processed_signal_before_detection(self):
        peak = [10] * 200 + [10 + int(400 * max(0.0, 1 - abs(i - 6) / 6)) for i in range(13)] + [10] * 200
        detector = dict(DETECTOR_DEFAULTS, smooth_passes=0)
        plain, scaled = {'values': list(peak)}, {'values': list(peak)}
        _measure_morphology_channel(plain, 50, None, detector)
        _measure_morphology_channel(scaled, 50, None, detector, 1.5)
        self.assertEqual(len(plain['peaks']), 1)
        self.assertEqual(scaled['peaks'][0]['height'], int(plain['peaks'][0]['height'] * 1.5))
        # threshold compares normalized heights: a 0.1 factor drops the peak
        low = {'values': list(peak)}
        _measure_morphology_channel(low, 50, None, detector, 0.1)
        self.assertEqual(low['peaks'], [])


class SGDegreeTests(unittest.TestCase):
    def test_weights_match_closed_forms(self):
        r = 7
        s2, s4, s6 = (2 * sum(k ** p for k in range(1, r + 1)) for p in (2, 4, 6))
        cubic = [(s6 * k - s4 * k ** 3) / (s2 * s6 - s4 * s4) for k in range(1, r + 1)]
        for a, b in zip(sg_derivative_weights(15, 3), cubic):
            self.assertAlmostEqual(a, b, places=12)
        self.assertEqual(sg_derivative_weights(15, 2), sg_derivative_weights(15, 1))
        for a, k in zip(sg_derivative_weights(15, 2), range(1, 8)):
            self.assertAlmostEqual(a, k / s2, places=12)

    def test_higher_degree_is_exact_for_quintic(self):
        f = lambda x: 3 + 2 * x - 0.5 * x ** 3 + 0.01 * x ** 5
        values = [int(round(f(x / 4) * 1000)) for x in range(-40, 41)]
        d5 = cubic_sg_derivative(values, window=11, degree=5)
        center = 40
        exact = 2 * 1000 / 4  # f'(0) per scan
        self.assertAlmostEqual(d5[center], exact, delta=0.5)
        with self.assertRaises(ValueError):
            cubic_sg_derivative(values, window=11, degree=11)


if __name__ == '__main__':
    unittest.main()
