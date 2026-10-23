"""Synthetic raw -> preparation -> SG -> filters -> ratio -> base SQ check."""
import math
import json
import unittest

from genemapper_open.preprocessing import prepare_trace
from genemapper_open.sg_peaks import detect_sg_peaks
from genemapper_open.standard_prefilters import filter_standard_peaks
from genemapper_open.ratio_sizing import match_ratio_standard
from genemapper_open.sizing_quality import evaluate_sizing_quality
from genemapper_open.sizing_pipeline import analyze_size_standard


class SizingPipelineTests(unittest.TestCase):
    def test_invented_fragments_are_recovered_from_known_linear_migration(self):
        # These are invented sizes, not a manufacturer standard definition.
        expected = [40, 70, 110, 145, 195, 260]
        centers = [200+20*bp for bp in expected]
        raw = [round(20+0.003*i+sum(1000*math.exp(-0.5*((i-center)/5)**2) for center in centers))
               for i in range(6501)]
        signal = prepare_trace(raw, window=51, spike_window=5, smooth_passes=2)['signal']
        peaks = detect_sg_peaks(signal)['peaks']
        self.assertEqual(len(peaks), len(expected))
        for peak, center in zip(peaks, centers):
            self.assertLessEqual(abs(peak['scan']-center), 1)
        filtered = filter_standard_peaks(peaks, len(expected), signal, group_gap=25)
        self.assertEqual(filtered['status'], 'passed')
        remaining = filtered['peaks']
        match = match_ratio_standard(remaining, expected, num_extra_peaks=25, allow_prefix=True)
        self.assertEqual(match['status'], 'accepted')
        assignments = {p['candidate_index']: p['expected_bp'] for p in match['matched']}
        selected = [dict(remaining[i], matched_bp=assignments.get(i))
                    for i in match['selection']['candidate_indices']]
        quality = evaluate_sizing_quality(selected, expected)
        self.assertEqual(quality['status'], 'evaluated')
        self.assertTrue(quality['complete'])
        self.assertGreaterEqual(quality['score'], 0.99)
        self.assertAlmostEqual(match['calibration'].size(200+20*180), 180, delta=0.1)
        pipeline = analyze_size_standard(raw, expected, num_extra_peaks=25, quality_threshold=0.75)
        self.assertEqual(pipeline['status'], 'accepted')
        self.assertEqual(pipeline['anchors'], match['anchors'])
        self.assertNotIn('calibration', pipeline['matching'])

    def test_quality_rejection_withholds_calibration_after_ratio_acceptance(self):
        expected = [40, 70, 110, 145, 195, 260]
        centers = [200+20*bp for bp in expected]
        heights = [1000, 15000, 1000, 1000, 1000, 1000]
        raw = [round(20+sum(height*math.exp(-0.5*((i-center)/5)**2)
                           for center, height in zip(centers, heights))) for i in range(6501)]
        result = analyze_size_standard(raw, expected, num_extra_peaks=25, quality_threshold=0.75)
        self.assertEqual(result['matching']['status'], 'accepted')
        self.assertEqual(result['base_quality']['score'], 0)
        self.assertIn('directed_neighbor_height_ratio_above10', result['base_quality']['reasons'])
        self.assertEqual(result['status'], 'rejected')
        self.assertEqual(result['anchors'], [])
        self.assertIsNone(result['calibration'])

    def test_invalid_quality_threshold_is_explicit(self):
        for threshold in (None, 0, float('nan'), True):
            result = analyze_size_standard([0]*31, [40, 70, 110], num_extra_peaks=25, quality_threshold=threshold)
            self.assertEqual(result['status'], 'rejected')
            self.assertEqual(result['stage'], 'validation')
            json.dumps(result, allow_nan=False)


if __name__ == '__main__':
    unittest.main()
