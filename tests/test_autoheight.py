"""Independent expectations for the Basic standard threshold mathematics."""
import random
import unittest

from genemapper_open.autoheight import (estimate_standard_autoheight,
    _average_stage, _polynomial_smooth, _directional_track, _trunc_div)
from genemapper_open.sizing_pipeline import analyze_size_standard


class AutoheightTests(unittest.TestCase):
    def test_constant_separates_control_signal_from_corrected_residual(self):
        # A constant background has zero corrected residual, but the wrapper
        # uses the background reconstructed by its second subtraction.
        result = estimate_standard_autoheight([100]*250)
        self.assertEqual(result['median_control'], 100)
        self.assertEqual(result['threshold'], 2500)
        self.assertEqual(result['control_signal'], [100]*250)

    def test_zero_floor_and_isolated_peak_on_positive_background(self):
        self.assertEqual(estimate_standard_autoheight([0]*800)['threshold'], 30)
        signal = [20]*1000
        signal[500] = 10020
        snapshot = list(signal)
        result = estimate_standard_autoheight(signal)
        self.assertEqual(result['median_control'], 20)
        self.assertEqual(result['threshold'], 500)
        self.assertEqual(signal, snapshot)
        self.assertTrue(all(0 <= control <= value for control, value in zip(result['control_signal'], signal)))

    def test_odd_and_even_average_against_direct_sum_identity(self):
        rng = random.Random(521)
        values = [rng.randrange(-15, 100) for _ in range(71)]
        for width in (3, 4, 5, 6, 21, 30, 62):
            radius = width//2
            for clip in (False, True):
                expected = list(values)
                for index in range(radius, len(values)-radius):
                    if index == radius:
                        total = sum(values[:width])
                    else:
                        total = sum(values[index-radius:index+radius+1])
                        if width % 2 == 0:
                            # The even-window recurrence preserves a fixed
                            # subtraction term, unlike a conventional box mean.
                            total -= values[width]
                    average = int(total/width)  # small exact sums; truncate0
                    expected[index] = min(values[index], average) if clip else average
                expected = [max(0, value) for value in expected]
                self.assertEqual(_average_stage(values, width, clip), expected)
        self.assertEqual(_average_stage([1,2,3,4,5,6,7,8,9], 4, False), [1,2,2,3,5,6,7,8,9])
        self.assertEqual(_average_stage([3,7], 11, True), [3,7])

    def test_polynomial_kernel_preserves_low_degree_polynomials(self):
        for power in (0, 1, 2, 3):
            values = [(index-40)**power for index in range(81)]
            self.assertEqual(_polynomial_smooth(values), values)
        self.assertEqual(_trunc_div(-7, 3), -2)

    def test_tracker_direction_and_strict_gap_boundaries(self):
        values = [0]*10 + [10, 11, 13, 22, 50]
        tracked = _directional_track(values)
        self.assertEqual(tracked[:12], [0]*10+[1,2])
        self.assertEqual(tracked[12:], [2,2,2])
        self.assertEqual(_directional_track(values, True), list(reversed(_directional_track(list(reversed(values))))))

    def test_public_input_bounds(self):
        for signal in (None, [], [0]*249, [0]*249+[True], [0]*249+[-1], [0]*249+[100000001]):
            with self.assertRaises(ValueError):
                estimate_standard_autoheight(signal)

    def test_pipeline_autoheight_is_explicit_and_keeps_matching_gates(self):
        result = analyze_size_standard([100]*300, [40,70,110], num_extra_peaks=25,
                                       quality_threshold=0.75, height_mode='basic_standard_auto')
        # Morphology preparation removes the constant baseline before autoheight.
        self.assertEqual(result['autoheight']['threshold'], 30)
        self.assertEqual(result['status'], 'rejected')
        self.assertEqual(result['anchors'], [])
        self.assertIsNone(result['calibration'])
        self.assertNotIn('control_signal', result['autoheight'])
        invalid = analyze_size_standard([0]*300, [40,70,110], num_extra_peaks=25,
                                       quality_threshold=0.75, height_mode='guess')
        self.assertEqual(invalid['stage'], 'validation')
        self.assertEqual(invalid['status'], 'rejected')


if __name__ == '__main__':
    unittest.main()
