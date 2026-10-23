"""Analytic distribution checks, QC boundaries and complete-standard validation."""
import copy
import json
import math
import unittest

from genemapper_open.sizing_quality import chi_square_survival, evaluate_sizing_quality


def matched_peaks(count=3, height=100.0, scan_step=1000.0, bp_step=50.0):
    return [{'scan': (i+1)*scan_step, 'height': height, 'begin_height': 0.0,
             'end_height': 0.0, 'matched_bp': (i+1)*bp_step} for i in range(count)]


class ChiSquareSurvivalTests(unittest.TestCase):
    def test_df1_matches_erfc(self):
        for statistic in (0, 1e-12, 0.1, 1, 3.841458820694124, 10, 100, 1000):
            with self.subTest(statistic=statistic):
                self.assertAlmostEqual(chi_square_survival(statistic, 1), math.erfc(math.sqrt(statistic/2)), places=13)

    def test_even_df_matches_closed_form_survival(self):
        for degrees in (2, 4, 6, 10, 20, 30):
            for statistic in (0, 0.01, 0.2, 2, 8, 20, 60, 100):
                value = statistic/2
                expected = math.exp(-value)*math.fsum(value**k/math.factorial(k) for k in range(degrees//2))
                with self.subTest(degrees=degrees, statistic=statistic):
                    self.assertAlmostEqual(chi_square_survival(statistic, degrees), expected, places=13)

    def test_extreme_tails_remain_direct_upper_tail(self):
        self.assertGreater(chi_square_survival(1000, 2), 0)
        self.assertAlmostEqual(math.log(chi_square_survival(1000, 2)), -500, places=12)
        self.assertEqual(chi_square_survival(float('inf'), 3), 0)
        self.assertEqual(chi_square_survival(1e308, 30), 0)
        self.assertEqual(chi_square_survival(5e-324, 30), 1)

    def test_odd_df_matches_erfc_recurrence(self):
        for degrees in (3, 5, 15, 29):
            for statistic in (0.01, 0.2, 2, 8, 20, 60, 100):
                value = statistic/2
                expected = math.erfc(math.sqrt(value))
                shape = 0.5
                while shape < degrees/2:
                    expected += math.exp(-value+shape*math.log(value)-math.lgamma(shape+1))
                    shape += 1
                with self.subTest(degrees=degrees, statistic=statistic):
                    self.assertAlmostEqual(chi_square_survival(statistic, degrees), expected, places=13)

    def test_monotonicity_and_invalid_parameters(self):
        for degrees in (1, 2, 3, 15, 30, 1024):
            values = [chi_square_survival(value, degrees) for value in (0, 0.1, 1, 10, 100, 1000, 1e6)]
            self.assertTrue(all(0 <= q <= 1 for q in values))
            self.assertTrue(all(right <= left for left, right in zip(values, values[1:])))
        for value, degrees in ((-1, 3), (float('nan'), 3), (True, 3), (1, 0), (1, 1.5), (1, True), (1, 1025)):
            with self.assertRaises(ValueError):
                chi_square_survival(value, degrees)


class SizingQualityTests(unittest.TestCase):
    def evaluate(self, peaks):
        expected = [peak['matched_bp'] for peak in peaks if peak.get('matched_bp') is not None]
        return evaluate_sizing_quality(peaks, expected)

    def test_uniform_complete_standard_has_unit_score(self):
        result = self.evaluate(matched_peaks())
        self.assertEqual(result['status'], 'evaluated')
        self.assertTrue(result['complete'])
        self.assertEqual(result['score'], 1)
        self.assertEqual(result['chi_square_statistic'], 0)
        self.assertEqual(result['degrees_of_freedom'], 1)
        self.assertFalse(result['original_runtime_validated'])

    def test_spacing_statistic_and_nonnegative_half_up(self):
        peaks = matched_peaks(count=4)
        for peak, scan in zip(peaks, (1000, 2000, 3500, 5000)):
            peak['scan'] = scan
        result = self.evaluate(peaks)
        rate = 150/4000
        statistic = sum((rate*gap-50)**2/50 for gap in (1000, 1500, 1500))
        expected = math.exp(-statistic/2)  # n-2=2
        self.assertAlmostEqual(result['chi_square_statistic'], statistic)
        self.assertAlmostEqual(result['raw_score'], expected)
        self.assertEqual(result['score'], math.floor(expected*10000+0.5)/10000)

    def test_boundary_penalties_are_independent_and_strict(self):
        peaks = matched_peaks(count=12)
        peaks[4]['begin_height'] = 30
        peaks[4]['end_height'] = 30
        result = self.evaluate(peaks)
        self.assertEqual(result['penalty_factor'], 0.25)
        self.assertEqual(result['score'], 0.25)
        self.assertEqual([event['kind'] for event in result['events']], ['begin_height_outlier', 'end_height_outlier'])
        peaks[4]['begin_height'] = 25
        peaks[4]['end_height'] = 25
        self.assertEqual(self.evaluate(peaks)['score'], 1)  # equality0.25 passes
        peaks = matched_peaks(count=10)
        peaks[4]['begin_height'] = 30
        self.assertEqual(self.evaluate(peaks)['score'], 1)  # equality10*mean passes

    def test_height_median_penalty_and_directional_ratio(self):
        peaks = matched_peaks(count=4)
        peaks[0]['height'] = 900
        result = self.evaluate(peaks)
        self.assertEqual(result['penalty_factor'], 0.5)
        self.assertEqual(result['score'], 0.5)  # falling height is not ratio>10
        peaks[0]['height'] = 800
        self.assertEqual(self.evaluate(peaks)['score'], 1)  # equality8*median passes
        peaks = matched_peaks()
        peaks[1]['height'] = 1000
        self.assertNotIn('directed_neighbor_height_ratio_above10', self.evaluate(peaks)['reasons'])
        peaks[1]['height'] = 1001
        self.assertIn('directed_neighbor_height_ratio_above10', self.evaluate(peaks)['reasons'])

    def test_unmatched_thresholds_are_strict_and_boundaries_excluded(self):
        base = matched_peaks()
        for height, factor, fatal in ((70, 1, False), (70.001, 0.9, False), (500, 0.9, False), (500.001, 1, True)):
            unmatched = {'scan': 1500, 'height': height, 'begin_height': 0, 'end_height': 0}
            peaks = [base[0], unmatched, *base[1:]]
            result = self.evaluate(peaks)
            with self.subTest(height=height):
                self.assertEqual(result['score'], 0 if fatal else factor)
                self.assertEqual(result['penalty_factor'], factor)
        outside = {'scan': 500, 'height': 100000, 'begin_height': 0, 'end_height': 0}
        self.assertEqual(self.evaluate([outside, *base])['score'], 1)

    def test_multiple_unmatched_penalties_multiply(self):
        base = matched_peaks()
        extra = [{'scan': scan, 'height': 80, 'begin_height': 0, 'end_height': 0} for scan in (1250, 1500)]
        result = self.evaluate([base[0], *extra, *base[1:]])
        self.assertAlmostEqual(result['penalty_factor'], 0.81)
        self.assertEqual(result['score'], 0.81)

    def test_rate_bound_accepts_equality(self):
        peaks = matched_peaks(scan_step=200)
        self.assertEqual(self.evaluate(peaks)['score'], 1)
        peaks = matched_peaks(scan_step=199)
        self.assertEqual(self.evaluate(peaks)['reasons'], ['global_migration_rate_above0_25'])

    def test_incomplete_and_exact_expected_coverage_are_distinct(self):
        peaks = matched_peaks()
        result = evaluate_sizing_quality(peaks, [50, 100, 150, 200])
        self.assertEqual(result['status'], 'evaluated')
        self.assertEqual(result['score'], 0)
        self.assertFalse(result['original_count_gate_passed'])
        result = evaluate_sizing_quality(peaks, [50, 100, 151])
        self.assertEqual(result['score'], 0)
        self.assertTrue(result['original_count_gate_passed'])
        self.assertFalse(result['complete'])
        self.assertIn('matched_sizes_do_not_equal_expected_standard', result['reasons'])

    def test_input_unchanged_json_serializable_and_negative_boundaries(self):
        peaks = matched_peaks()
        peaks[0]['begin_height'] = -10
        previous = copy.deepcopy(peaks)
        result = self.evaluate(peaks)
        self.assertEqual(peaks, previous)
        json.dumps(result, allow_nan=False)
        self.assertEqual(result['score'], 1)

    def test_invalid_features_sizes_order_and_limits(self):
        good = matched_peaks()
        for peaks, expected in ((None, [1, 2, 3]), (good*1366, [50, 100, 150]),
                                (good, [1, 1, 3]), (good, [1, 2]), (good[::-1], [50, 100, 150])):
            result = evaluate_sizing_quality(peaks, expected)
            self.assertEqual(result['status'], 'rejected')
            self.assertIsNone(result['score'])
        for field, value in (('height', 0), ('height', float('nan')), ('begin_height', True),
                             ('scan', 10**10000), ('matched_bp', -1), ('end_height', float('inf'))):
            peaks = copy.deepcopy(good)
            peaks[0][field] = value
            result = self.evaluate(peaks)
            self.assertEqual(result['status'], 'rejected')
            self.assertIsNone(result['score'])


if __name__ == '__main__':
    unittest.main()
