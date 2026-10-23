"""Brute-force objective comparisons and diagnostic boundary cases."""
import itertools
import random
import unittest

from genemapper_open.ratio_sizing import match_ratio_standard


def enumerate_scores(scans, heights, bp, weight):
    results = []
    scale = max(heights)
    for path in itertools.combinations(range(len(scans)), len(bp)):
        geometry = 0.0
        for k in range(2, len(path)):
            a, b, c = (scans[path[i]] for i in (k-2, k-1, k))
            x = (c-b)/(b-a)
            y = (bp[k]-bp[k-1])/(bp[k-1]-bp[k-2])
            geometry += min(x/y, y/x)
        score = ((1-weight)*sum(heights[i]/scale for i in path)+weight*geometry)/(len(bp)-2*weight)
        results.append((score, path))
    return sorted(results, key=lambda item: (-item[0], item[1]))


class RatioSizingTests(unittest.TestCase):
    def match(self, scans, heights, bp, **kwargs):
        return match_ratio_standard([{'scan': x, 'height': h} for x, h in zip(scans, heights)], bp,
                                   num_extra_peaks=kwargs.pop('num_extra_peaks', 25), **kwargs)

    def test_best_and_runner_up_against_exhaustive_search(self):
        rng = random.Random(312)
        for n, count, weight in itertools.product((3, 4, 5), (6, 8), (0.0, 0.6, 0.8, 1.0)):
            scans = sorted(rng.sample(range(100, 9000), count))
            heights = [rng.randrange(100, 1000) for _ in scans]
            bp = sorted(rng.sample(range(20, 500), n))
            clip = sorted(heights)[-n]
            clipped = [min(h, clip) for h in heights]
            expected = enumerate_scores(scans, clipped, bp, weight)
            result = self.match(scans, heights, bp, ratio_factor=weight, max_bp_per_scan=None)
            with self.subTest(n=n, count=count, weight=weight):
                self.assertAlmostEqual(result['diagnostics']['best_score'], expected[0][0], places=12)
                self.assertAlmostEqual(result['diagnostics']['second_best_score'], expected[1][0], places=12)
                path = tuple(p['candidate_index'] for p in result['matched'])
                actual_score = next(score for score, candidate in expected if candidate == path)
                self.assertAlmostEqual(actual_score, expected[0][0], places=12)

    def test_linear_unique_standard_has_score_one(self):
        result = self.match([1000, 2000, 4000, 7000], [500]*4, [50, 100, 200, 350])
        self.assertEqual(result['status'], 'accepted')
        self.assertAlmostEqual(result['diagnostics']['best_score'], 1.0)
        self.assertIsNone(result['diagnostics']['second_best_score'])
        self.assertEqual(len(result['anchors']), 4)

    def test_height_unit_scale_invariance_including_subnormal_input(self):
        for height in (100, 1e12, 1e-200, 1e-323, 5e-324):
            with self.subTest(height=height):
                result = self.match([1000, 2000, 3000], [height]*3, [50, 100, 150])
                self.assertEqual(result['status'], 'accepted')
                self.assertAlmostEqual(result['diagnostics']['best_score'], 1)

    def test_perfect_line_at_score_threshold_one_remains_complete(self):
        for weight in (0.1, 0.3, 0.7, 0.95):
            for length in (3, 4, 11, 25, 32):
                result = self.match([1000*i for i in range(length)], [100]*length,
                                    [50*(i+1) for i in range(length)], ratio_factor=weight,
                                    min_accepted_score=1, allow_prefix=True, num_extra_peaks=0)
                self.assertEqual(result['status'], 'accepted')
                self.assertEqual(len(result['attempts']), 1)
                self.assertTrue(result['diagnostics']['complete'])

    def test_equal_score_alternative_is_rejected(self):
        result = self.match([1000, 2000, 3000, 4000], [500]*4, [50, 100, 150], ambiguity_margin=0)
        self.assertEqual(result['status'], 'rejected')
        self.assertIn('Ambiguous', result['reasons'][0])
        self.assertEqual(result['anchors'], [])
        self.assertIsNone(result['calibration'])

    def test_selection_tie_cutoff_favors_later_indices(self):
        result = self.match([1000, 2000, 3000, 4000, 5000], [100]*5, [50, 100, 150], num_extra_peaks=0)
        self.assertEqual(result['selection']['candidate_indices'], [2, 3, 4])
        self.assertEqual(result['selection']['height_clip'], 100)

    def test_height_clipping_limits_large_artifact(self):
        result = self.match([1000, 2000, 3000, 4000], [1000000, 100, 100, 100], [50, 100, 150])
        self.assertEqual(result['selection']['height_clip'], 100)
        self.assertAlmostEqual(result['diagnostics']['best_score'], 1)
        self.assertEqual(result['status'], 'rejected')  # clipping exposes a tie

    def test_prefix_is_diagnostic_and_never_calibration(self):
        result = self.match([1000, 2000, 3000, 4000], [100]*4, [50, 100, 150, 600],
                            allow_prefix=True, min_accepted_score=0.95)
        self.assertEqual([a['expected_prefix_count'] for a in result['attempts']], [4, 3])
        self.assertEqual([p['expected_bp'] for p in result['matched']], [50, 100, 150])
        self.assertEqual(result['status'], 'rejected')
        self.assertFalse(result['diagnostics']['complete'])
        self.assertIsNone(result['calibration'])

    def test_strict_search_does_not_fall_back(self):
        result = self.match([1000, 2000, 3000, 4000], [100]*4, [50, 100, 150, 600], min_accepted_score=0.95)
        self.assertEqual(len(result['attempts']), 1)
        self.assertIn('No attempted', result['reasons'][0])

    def test_fallback_keeps_prefix_and_can_reach_pair(self):
        result = self.match([1000, 2000, 3000, 4000], [100]*4, [50, 100, 500, 550],
                            allow_prefix=True, min_accepted_score=0.95)
        self.assertEqual([a['expected_prefix_count'] for a in result['attempts']], [4, 3, 2])
        self.assertEqual([p['expected_bp'] for p in result['matched']], [50, 100])
        self.assertEqual(result['status'], 'rejected')
        self.assertEqual(result['anchors'], [])

    def test_pure_geometry_does_not_divide_by_zero_at_pair(self):
        result = self.match([1000, 2000, 3000], [100]*3, [50, 100, 500],
                            ratio_factor=1, allow_prefix=True, min_accepted_score=0.95)
        self.assertEqual([a['expected_prefix_count'] for a in result['attempts']], [3])
        self.assertEqual(result['status'], 'rejected')

    def test_migration_rate_gate_and_equality(self):
        scans, bp = [100, 200, 400], [25, 50, 100]
        self.assertEqual(self.match(scans, [100]*3, bp)['status'], 'accepted')
        result = self.match(scans, [100]*3, [26, 52, 104])
        self.assertIn('migration-rate', result['reasons'][0])
        self.assertEqual(result['anchors'], [])

    def test_sorted_input_is_normalized_without_mutation(self):
        peaks = [{'scan': 3000, 'height': 200}, {'scan': 1000, 'height': 200}, {'scan': 2000, 'height': 200}]
        snapshot = [dict(p) for p in peaks]
        result = match_ratio_standard(peaks, [50, 100, 150], num_extra_peaks=0)
        self.assertEqual(peaks, snapshot)
        self.assertEqual([p['candidate_index'] for p in result['matched']], [1, 2, 0])

    def test_invalid_inputs_and_resource_limits(self):
        good = [{'scan': i*1000, 'height': 100} for i in range(4)]
        cases = [({'ratio_factor': -1}), ({'ratio_factor': float('nan')}), ({'ratio_factor': True}),
                 ({'min_accepted_score': 0}), ({'ambiguity_margin': -1}), ({'allow_prefix': 1}),
                 ({'num_extra_peaks': 33}), ({'max_bp_per_scan': 0})]
        for options in cases:
            result = match_ratio_standard(good, [50, 100, 150], num_extra_peaks=options.pop('num_extra_peaks', 25), **options)
            self.assertEqual(result['status'], 'rejected')
            self.assertEqual(result['anchors'], [])
        for peaks, bp in [([], [1, 2, 3]), (good*1025, [1, 2, 3]), (good, [1, 1, 2]),
                          ([{'scan': 1, 'height': 0}]*3, [1, 2, 3]), (good[:2], [1, 2, 3]),
                          ([{'scan': 1, 'height': 100}]*3, [1, 2, 3]),
                          (good, [1e-300, 2e-300, 3e-300]),
                          ([{'scan': 10**10000, 'height': 100}]*3, [1, 2, 3])]:
            self.assertEqual(match_ratio_standard(peaks, bp, num_extra_peaks=25)['status'], 'rejected')


if __name__ == '__main__':
    unittest.main()
