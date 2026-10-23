"""Analytic rules/boundaries for independent sizing candidate prefilters."""
from copy import deepcopy
import unittest

from genemapper_open.standard_prefilters import filter_standard_peaks


def peak(scan, height=100, begin_height=0, end_height=0, width=2, **extra):
    return {'scan': scan, 'height': height, 'begin': max(0, scan-3), 'end': scan+3,
            'begin_height': begin_height, 'end_height': end_height, 'fwhm_scan': width, **extra}


class StandardPrefilterTests(unittest.TestCase):
    def run_filter(self, peaks, count=3, signal=None, **kwargs):
        if signal is None:
            signal = [0]*1000
        return filter_standard_peaks(peaks, count, signal, **kwargs)

    def test_preserves_fields_and_input_without_forcing_expected_count(self):
        peaks = [peak(scan, candidate_index=i, metadata={'a': i}) for i, scan in enumerate((100, 200, 300, 400))]
        before = deepcopy(peaks)
        result = self.run_filter(peaks)
        self.assertEqual(result['status'], 'passed')
        self.assertEqual(result['peaks'], peaks)
        self.assertEqual(peaks, before)
        self.assertIsNot(result['peaks'][0], peaks[0])

    def test_large_early_peak_cuts_entire_preceding_candidate_prefix(self):
        peaks = [peak(10), peak(50, 2000), peak(200), peak(300), peak(400)]
        result = self.run_filter(peaks)
        self.assertEqual([p['scan'] for p in result['peaks']], [200, 300, 400])
        self.assertEqual(result['diagnostics']['early_high_removed_indices'], [1])
        self.assertEqual(result['diagnostics']['early_prefix_removed_indices'], [0])

    def test_shape_peak_removed_even_below_large_height_threshold(self):
        result = self.run_filter([peak(10, begin_height=80), peak(100), peak(200), peak(300)])
        self.assertEqual(result['diagnostics']['early_shape_removed_indices'], [0])
        self.assertEqual(result['diagnostics']['last_large_early_scan'], 0)

    def test_early_height_width_and_shape_equalities_are_strict(self):
        for first in (peak(10, 1000), peak(10, 500, width=5), peak(10, 600, width=4),
                      peak(10, begin_height=70, end_height=70)):
            result = self.run_filter([first, peak(100), peak(200), peak(300)])
            self.assertIn(10, [p['scan'] for p in result['peaks']])
        result = self.run_filter([peak(10, 600, width=5), peak(100), peak(200), peak(300)])
        self.assertEqual(result['diagnostics']['early_high_removed_indices'], [0])

    def test_zero_scan_is_cut_when_no_large_artifact_was_seen(self):
        result = self.run_filter([peak(0), peak(100), peak(200), peak(300)])
        self.assertEqual(result['diagnostics']['early_prefix_removed_indices'], [0])

    def test_insufficient_reference_skips_only_early_stage(self):
        peaks = [peak(100), peak(110, 1000), peak(120), peak(300, 1000), peak(500, 1000)]
        result = self.run_filter(peaks)
        self.assertFalse(result['diagnostics']['early_stage_applied'])
        self.assertEqual(result['diagnostics']['late_reference_count'], 2)
        self.assertTrue(result['diagnostics']['group_removed_indices'])

    def test_reference_count_equality_and_cutoff_peak_handling(self):
        result = self.run_filter([peak(100), peak(200), peak(300), peak(400)], count=4)
        self.assertEqual(result['diagnostics']['late_reference_count'], 3)
        self.assertTrue(result['diagnostics']['early_stage_applied'])
        result = self.run_filter([peak(100), peak(200, begin_height=80), peak(300), peak(400), peak(500)], count=4)
        self.assertEqual(result['diagnostics']['early_cutoff_scan'], 200)
        self.assertEqual(result['diagnostics']['late_reference_count'], 4)
        self.assertEqual(result['diagnostics']['early_shape_removed_indices'], [1])

    def test_group_left_priority_and_examined_peaks_do_not_force_n(self):
        peaks = [peak(100, 599), peak(110, 1000), peak(120), peak(300, 1000), peak(500, 1000)]
        result = self.run_filter(peaks)
        self.assertEqual(result['diagnostics']['group_removed_indices'], [0])
        self.assertEqual(result['diagnostics']['group_events'][0]['side'], 'left')
        self.assertIn(120, [p['scan'] for p in result['peaks']])
        self.assertEqual(len(result['peaks']), 4)

    def test_group_ratio_and_distance_equalities_do_not_remove_left(self):
        for left in (peak(100, 600), peak(85, 599)):
            peaks = [left, peak(110, 1000), peak(120), peak(300, 1000), peak(500, 1000)]
            result = self.run_filter(peaks)
            self.assertNotIn(0, result['diagnostics']['group_removed_indices'])
            self.assertIn(2, result['diagnostics']['group_removed_indices'])

    def test_directional_shape_can_remove_ratio_equal_point_six(self):
        peaks = [peak(100, 600, end_height=500), peak(110, 1000), peak(120), peak(300, 1000), peak(500, 1000)]
        result = self.run_filter(peaks)
        self.assertEqual(result['diagnostics']['group_removed_indices'], [0])

    def test_offscale_guard_is_on_dominant_not_neighbor(self):
        common = [peak(100, 599), peak(110, 1000), peak(120), peak(300, 1000), peak(500, 1000)]
        dominant_flag = deepcopy(common)
        dominant_flag[1]['offscaled'] = True
        self.assertEqual(self.run_filter(dominant_flag)['diagnostics']['group_removed_indices'], [])
        neighbor_flag = deepcopy(common)
        neighbor_flag[0]['offscaled'] = True
        self.assertEqual(self.run_filter(neighbor_flag)['diagnostics']['group_removed_indices'], [0])

    def connectivity_peaks(self, previous_end=100, current_begin=140, reference_begin=1):
        first = peak(150, 100, begin_height=40)
        first['begin'] = current_begin
        artifact = peak(50, 20000)
        artifact['end'] = previous_end
        return [peak(10), artifact, first]+[peak(s, 1000, begin_height=reference_begin) for s in (250, 350, 450)]

    def test_connected_tail_is_removed_only_if_all_copied_samples_reach_begin_height(self):
        signal = [40]*1000
        result = self.run_filter(self.connectivity_peaks(), signal=signal)
        self.assertEqual(result['diagnostics']['connected_tail_removed_indices'], [2])
        signal[120] = 39
        result = self.run_filter(self.connectivity_peaks(), signal=signal)
        self.assertEqual(result['diagnostics']['connected_tail_removed_indices'], [])

    def test_connected_tail_equal_bounds_and_zero_reference_mean(self):
        result = self.run_filter(self.connectivity_peaks(previous_end=140, reference_begin=0))
        self.assertEqual(result['diagnostics']['connected_tail_removed_indices'], [2])

    def test_malformed_connectivity_interval_is_explicit_rejection(self):
        result = self.run_filter(self.connectivity_peaks(previous_end=170), signal=[40]*1000)
        self.assertEqual(result['status'], 'rejected')
        self.assertIn('Malformed', result['reasons'][0])
        self.assertEqual(result['peaks'], [])

    def test_zero_length_literal_connectivity_copy_remains_empty_and_connected(self):
        result = self.run_filter(self.connectivity_peaks(previous_end=140.5, current_begin=139.8))
        self.assertEqual(result['status'], 'passed')
        self.assertEqual(result['diagnostics']['connected_tail_removed_indices'], [2])

    def test_invalid_inputs_and_initial_count_guard(self):
        valid = [peak(100), peak(200), peak(300)]
        for peaks, count, signal in ((valid[:2], 3, [0]*1000), (valid, True, [0]*1000),
                                    (list(reversed(valid)), 3, [0]*1000), (valid, 3, [0.0]*1000),
                                    (valid, 3, []), ([peak(100, 0)]+valid[1:], 3, [0]*1000)):
            with self.subTest(count=count, n=len(peaks)):
                result = self.run_filter(peaks, count, signal)
                self.assertEqual(result['status'], 'rejected')
                self.assertEqual(result['peaks'], [])


if __name__ == '__main__':
    unittest.main()
