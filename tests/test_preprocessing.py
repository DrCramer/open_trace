"""Mathematical oracle and analytic examples, independent of vendor runtime."""
import itertools
import math
import random
import unittest

from genemapper_open.preprocessing import estimate_baseline, prepare_trace, smooth_trace


def direct_baseline(values, window, spike, smooth):
    def filt(trace, width, operation):
        r = width//2
        return [operation(trace[max(0, i-r):min(len(trace), i+r+1)]) for i in range(len(trace))]
    def opening(trace, width):
        return filt(filt(trace, width, min), width, max)
    def closing(trace, width):
        return filt(filt(trace, width, max), width, min)
    a = closing(opening(values, window), spike)
    b = opening(closing(values, spike), window)
    average = [(x+y)/2 for x, y in zip(a, b)]
    return filt(average, smooth, lambda items: math.fsum(items)/len(items))


class TracePreparationTests(unittest.TestCase):
    def test_baseline_against_direct_window_oracle(self):
        rng = random.Random(171)
        for length, window, spike, smooth in itertools.product((1, 2, 5, 17, 71), (1, 3, 7, 51), (1, 3, 5), (1, 3, 9)):
            values = [rng.randrange(-100, 1000) for _ in range(length)]
            with self.subTest(length=length, window=window, spike=spike, smooth=smooth):
                actual = estimate_baseline(values, window=window, spike_window=spike, smoothing_window=smooth)
                expected = direct_baseline(values, window, spike, smooth)
                for x, y in zip(actual, expected):
                    self.assertAlmostEqual(x, y, places=10)

    def test_constant_signed_signal_is_removed(self):
        for level in (-32768, -10, 0, 100, 32767):
            result = prepare_trace([level]*81, window=51, spike_window=5, smooth_passes=2)
            self.assertEqual(result['baseline'], [float(level)]*81)
            self.assertEqual(result['signal'], [0]*81)

    def test_broad_baseline_and_narrow_peak(self):
        values = [100]*101
        values[50] = 1100
        result = prepare_trace(values, window=51, spike_window=5, smooth_passes=0)
        self.assertEqual(result['baseline'], [100.0]*101)
        self.assertEqual(result['signal'][50], 1000)
        self.assertEqual(sum(result['signal']), 1000)

    def test_triangular_impulse_known_values_and_endpoints(self):
        signal = [0, 0, 16, 0, 0]
        self.assertEqual(smooth_trace(signal, passes=1), [0, 4, 8, 4, 0])
        self.assertEqual(smooth_trace(signal, passes=2), [0, 4, 6, 4, 0])
        self.assertEqual(smooth_trace([7, 100, 19], passes=1), [7, 56, 19])
        self.assertEqual(smooth_trace([7, 19], passes=2), [7, 19])

    def test_smoothing_floor_and_unbounded_addition_contract(self):
        self.assertEqual(smooth_trace([-1, 0, 0], passes=1), [-1, -1, 0])
        self.assertEqual(smooth_trace([2**31-1]*3, passes=2), [2**31-1]*3)

    def test_nonnegative_half_up_and_input_unchanged(self):
        rounded = prepare_trace([0, 1, 0], window=3, spike_window=3)
        self.assertEqual(rounded['baseline'], [0.5]*3)
        self.assertEqual(rounded['corrected'], [0, 1, 0])
        values = [4, 0, 1, 0, 3, 0, 7]
        copy = list(values)
        result = prepare_trace(values, window=3, spike_window=3)
        baseline = direct_baseline(values, 3, 3, 1)
        self.assertEqual(result['corrected'], [math.floor(max(0, x-y)+0.5) for x, y in zip(values, baseline)])
        self.assertEqual(values, copy)
        self.assertIsNot(result['signal'], values)
        self.assertTrue(all(type(x) is int and x >= 0 for x in result['signal']))

    def test_invalid_data_and_settings_fail_explicitly(self):
        for values in ([], [True], [1.5], [float('nan')], [2**31], '123'):
            with self.subTest(values=values), self.assertRaises(ValueError):
                estimate_baseline(values, window=3, spike_window=1)
        for width in (0, 2, -1, True, 100003):
            with self.subTest(width=width), self.assertRaises(ValueError):
                estimate_baseline([1, 2, 3], window=width, spike_window=1)
        for passes in (-1, 33, True, 1.5):
            with self.subTest(passes=passes), self.assertRaises(ValueError):
                smooth_trace([1, 2, 3], passes=passes)

    def test_residual_exceeding_signed_int_is_rejected(self):
        with self.assertRaises(ValueError):
            prepare_trace([-(2**31), 2**31-1, -(2**31)], window=3, spike_window=1)


if __name__ == '__main__':
    unittest.main()
