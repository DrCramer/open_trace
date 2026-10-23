import json
import unittest
from genemapper_open.marker_quality import score_negative_marker


class NegativeMarkerQualityTests(unittest.TestCase):
    def score(self, zero, spike, **kwargs):
        return score_negative_marker(kwargs.get('confidences', [.6, .8]), zero_peak_value=zero,
            spike_metric=spike, spike_weight=kwargs.get('weight', .5), sample_sq=.75)

    def test_zero_peak_and_spike_exact_state_matrix(self):
        for zero in (-2, -1, 0, 1, .5):
            for spike in (-2, -1, 0, 1):
                row = self.score(zero, spike)
                self.assertEqual(row['cc'], int(zero==0))
                self.assertEqual(row['gq'], 0 if zero==0 else .5 if spike==1 else 1)

    def test_negative_failure_preserves_individual_confidence_then_sq(self):
        row = self.score(0, 1)
        self.assertEqual(row['gq'], 0)
        self.assertFalse(row['ordinary_confidence_assignment_applied'])
        self.assertEqual(row['call_confidence_before_sq'], [.6, .8])
        self.assertAlmostEqual(row['call_confidence_after_sq'][0], .45)
        self.assertAlmostEqual(row['call_confidence_after_sq'][1], .6)

    def test_empty_calls_and_weight_boundaries(self):
        row = self.score(1, 1, confidences=[])
        self.assertEqual(row['gq'], .5)
        self.assertEqual(row['call_confidence_after_sq'], [])
        json.dumps(row, allow_nan=False)
        self.assertEqual(self.score(1, 1, weight=0)['gq'], 1)
        self.assertEqual(self.score(1, 1, weight=1)['gq'], 0)

    def test_invalid_inputs(self):
        for zero, spike in ((True, 0), (1, True), (1, .5), (float('nan'), 0)):
            with self.assertRaises(ValueError): self.score(zero, spike)
        for confidences in ([True], [1.1], [float('inf')]):
            with self.assertRaises(ValueError): self.score(1, 0, confidences=confidences)
