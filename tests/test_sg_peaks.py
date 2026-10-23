"""Analytic derivative/width examples and explicit boundary behavior."""
import math
import unittest

from genemapper_open.sg_peaks import cubic_sg_derivative, detect_sg_peaks, _half_width


class SGPeakTests(unittest.TestCase):
    def test_derivative_reproduces_cubic_polynomials_and_zeros_edges(self):
        for window in (5, 9, 15, 31):
            for power in (0, 1, 2, 3):
                values = [(i-40)**power for i in range(81)]
                derivative = cubic_sg_derivative(values, window=window)
                r = window//2
                self.assertEqual(derivative[:r], [0.0]*r)
                self.assertEqual(derivative[-r:], [0.0]*r)
                for i in range(r, 81-r):
                    expected = power*(i-40)**(power-1) if power else 0
                    self.assertAlmostEqual(derivative[i], expected, places=9)

    def test_gaussian_height_area_and_width(self):
        sigma = 8
        values = [round(1000*math.exp(-0.5*((i-100)/sigma)**2)) for i in range(201)]
        result = detect_sg_peaks(values)
        self.assertEqual(len(result['peaks']), 1)
        peak = result['peaks'][0]
        self.assertEqual(peak['scan'], 100)
        self.assertEqual(peak['height'], 1000)
        self.assertAlmostEqual(peak['fwhm_scan'], 2*math.sqrt(2*math.log(2))*sigma, delta=0.05)
        self.assertEqual(peak['area'], sum(values[math.ceil(peak['begin']):math.floor(peak['end'])+1]))

    def test_plateau_snap_is_first_maximum(self):
        values = [0]*50+[10, 30, 70, 100, 100, 100, 70, 30, 10]+[0]*50
        result = detect_sg_peaks(values, window=5, threshold=100)
        self.assertEqual([p['scan'] for p in result['peaks']], [53])
        self.assertGreater(result['peaks'][0]['sg_scan'], 53)
        self.assertEqual(detect_sg_peaks(values, window=5, threshold=101)['peaks'], [])

    def test_no_implicit_minimum_distance_pruning(self):
        values = [0]*70
        values[25:30] = [0, 100, 200, 100, 0]
        values[32:37] = [0, 100, 200, 100, 0]
        result = detect_sg_peaks(values, window=5, threshold=100)
        self.assertEqual([p['scan'] for p in result['peaks']], [27, 34])

    def test_offscale_uses_inclusive_interval_overlap_and_preserves_input(self):
        values = [0]*50+[10, 30, 70, 100, 70, 30, 10]+[0]*50
        snapshot = values[:]
        peak = detect_sg_peaks(values, window=5, threshold=100)['peaks'][0]
        result = detect_sg_peaks(values, window=5, threshold=100,
                                 offscale_intervals=[[peak['begin'], peak['begin']]])
        self.assertTrue(result['peaks'][0]['offscaled'])
        self.assertEqual(values, snapshot)

    def test_ordinary_half_width_and_truncated_level_adjustment(self):
        self.assertEqual(_half_width([0, 5, 10, 5, 0], 0, 4, 2), 2)
        # Strict last-above/first-below interpolation skips the equal level8
        # sample: it joins (1,10) to (3,4), rather than returning scan2.
        self.assertAlmostEqual(_half_width([8, 10, 8, 4, 0], 0, 4, 1), 5/3)
        self.assertEqual(_half_width([0, 0, 0], 0, 2, 1), 0)

    def test_constant_and_monotone_signals_have_no_peak(self):
        for values in ([100]*101, list(range(101)), list(range(101, 0, -1))):
            self.assertEqual(detect_sg_peaks(values, threshold=1)['peaks'], [])

    def test_explicit_invalid_settings(self):
        for values, settings in [([], {}), (None, {'offscale_intervals': [[0, 1]]}),
                                 ([1]*20, {'window': 4}), ([True]*20, {}),
                                 ([1]*20, {'tau_begin': -1}), ([1]*20, {'tau_end': 1}),
                                 ([1]*20, {'threshold': 0}), ([1]*20, {'offscale_intervals': [[0, 20]]})]:
            with self.subTest(settings=settings), self.assertRaises(ValueError):
                detect_sg_peaks(values, **settings)


if __name__ == '__main__':
    unittest.main()
