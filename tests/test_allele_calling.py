"""Synthetic equations, boundary and review-state tests; no vendor kit data."""
from copy import deepcopy
import math
import unittest

from genemapper_open.allele_calling import call_sample


def kit_definition(**parameters):
    effective = {'ladder_cutoff_ratio': 0.01, 'cutoff_ratio': 0,
                 'minus_a_ratio': 0, 'minus_a_min_bp': 0, 'minus_a_max_bp': 0,
                 'backward_stutter_ranges': [], 'forward_stutter_ranges': [],
                 'clear_ranges': []}
    effective.update(parameters)
    return {'schema_version': 1, 'name': 'Synthetic mathematics only', 'markers': [{
        'name': 'SyntheticM', 'channel': 'DATA1', 'min_bp': 90, 'max_bp': 130,
        'repeat': 4, 'bins': [
            {'label': str(index), 'center_bp': size, 'left_bp': 0.5, 'right_bp': 0.5}
            for index, size in enumerate((100, 104, 108, 112), 1)],
        'ladder_labels': ['1', '2', '3', '4'], 'parameters': effective}]}


def offsets(kit, delta=0):
    return {'status': 'accepted', 'markers': {
        m['name']: {'status': 'accepted', 'min_offset_bp': delta, 'max_offset_bp': delta,
                    'bin_offsets': {b['label']: delta for b in m['bins']}}
        for m in kit['markers']}}


def peak(size, height=100, **kwargs):
    return {'channel': 'DATA1', 'scan': size * 10, 'size_bp': size,
            'height': height, **kwargs}


class AlleleCallingTests(unittest.TestCase):
    def test_explicit_forward_fill_records_derived_offsets_without_zero(self):
        kit=kit_definition();ladder=offsets(kit,1)
        ladder['markers']['SyntheticM']['bin_offsets']={'2':1,'4':2}
        result=self.call([peak(99),peak(103),peak(107),peak(110)],kit,ladder,missing_offset_policy='forward_fill')
        marker=result['markers'][0]
        self.assertEqual([c['label'] for c in marker['calls']],['1','2','3','4'])
        by_bin=marker['offsets']['by_bin']
        self.assertEqual(by_bin['1']['origin'],'virtual_min')
        self.assertEqual(by_bin['3']['origin'],'forward_fill')
        self.assertEqual(by_bin['3']['offset_bp'],1)
        self.assertEqual(by_bin['4']['offset_bp'],2)
        self.assertTrue(any(e['reason']=='derived_bin_offset' for e in marker['events']))
        self.assertNotIn('1',ladder['markers']['SyntheticM']['bin_offsets'])

    def test_forward_fill_requires_accepted_virtual_bounds_and_canonical_keys(self):
        kit=kit_definition()
        for changes in ({'min_offset_bp':None},{'bin_offsets':{}},{'bin_offsets':{'unknown':1}}):
            ladder=offsets(kit);ladder['markers']['SyntheticM'].update(changes)
            result=self.call([peak(100)],kit,ladder,missing_offset_policy='forward_fill')
            self.assertEqual(result['status'],'rejected')
            self.assertEqual(result['markers'][0]['calls'],[])

    def call(self, peaks, kit=None, ladder=None, **kwargs):
        kit = kit_definition() if kit is None else kit
        ladder = offsets(kit) if ladder is None else ladder
        return call_sample(peaks, kit, ladder, sizing_accepted=kwargs.pop('sizing_accepted', True), **kwargs)

    def marker(self, peaks, kit=None, **kwargs):
        result = self.call(peaks, kit, **kwargs)
        self.assertEqual(result['status'], 'tentative', result)
        return result['markers'][0]

    def test_calls_are_always_tentative_and_preserve_input(self):
        peaks = [peak(104, 250, flags=['review']), peak(100, 100)]
        kit = kit_definition()
        ladder = offsets(kit)
        before = deepcopy((peaks, kit, ladder))
        result = self.call(peaks, kit, ladder, source_metadata={'sha256': 'supplied'})
        self.assertEqual(result['status'], 'tentative')
        self.assertTrue(result['research_only'])
        self.assertFalse(result['external_validation'])
        self.assertFalse(result['original_runtime_validated'])
        self.assertEqual((peaks, kit, ladder), before)
        calls = result['markers'][0]['calls']
        self.assertEqual([c['label'] for c in calls], ['1', '2'])
        self.assertEqual([c['peak_index'] for c in calls], [1, 0])
        self.assertTrue(all(c['review_required'] for c in calls))
        calls[1]['flags'].append('changed')
        self.assertEqual(peaks[0]['flags'], ['review'])
        self.assertEqual(result['source_metadata'], {'sha256': 'supplied'})

    def test_cutoff_equality_is_excluded(self):
        marker = self.marker([peak(100, 25), peak(104, 100)], kit_definition(cutoff_ratio=0.25))
        self.assertEqual([c['size_bp'] for c in marker['calls']], [104])
        self.assertEqual(marker['peak_review'][0]['state'], 'below_or_equal_cutoff')
        self.assertEqual(marker['cutoff_height'], 25)
        self.assertEqual(marker['stutter_calls'], [])

    def test_cutoff_one_can_remove_all_candidates_without_forcing_a_call(self):
        marker = self.marker([peak(100)], kit_definition(cutoff_ratio=1))
        self.assertEqual(marker['calls'], [])

    def test_minus_a_strict_ratio_inclusive_distance_and_separate_review(self):
        kit = kit_definition(minus_a_ratio=0.5, minus_a_min_bp=1, minus_a_max_bp=2)
        for distance in (1, 2):
            marker = self.marker([peak(100, 49), peak(100 + distance, 100)], kit)
            self.assertEqual([c['peak_index'] for c in marker['calls']], [1])
            self.assertEqual(marker['peak_review'][0]['state'], 'minus_a')
            self.assertEqual(marker['stutter_calls'], [])
        equality = self.marker([peak(100, 50), peak(101, 100)], kit)
        self.assertEqual(len(equality['calls']), 2)
        for distance in (0.5, 3):
            outside = self.marker([peak(100, 1), peak(100 + distance, 100)], kit)
            self.assertEqual(len(outside['calls']), 2)

    def test_minus_a_parent_must_still_be_eligible(self):
        kit = kit_definition(cutoff_ratio=0.5, minus_a_ratio=1,
                             minus_a_min_bp=1, minus_a_max_bp=2)
        marker = self.marker([peak(100, 60), peak(101, 10), peak(108, 100)], kit)
        self.assertEqual([c['size_bp'] for c in marker['calls']], [100, 108])

    def test_backward_stutter_ratio_is_strict_and_endpoints_inclusive(self):
        kit = kit_definition(backward_stutter_ranges=[{'min_bp': 3, 'max_bp': 5, 'ratio': 0.2}])
        for distance in (3, 5):
            marker = self.marker([peak(100, 19), peak(100 + distance, 100)], kit)
            self.assertEqual([c['peak_index'] for c in marker['calls']], [1])
            self.assertEqual([c['peak_index'] for c in marker['stutter_calls']], [0])
            self.assertEqual(marker['peak_review'][0]['state'], 'backward_stutter')
        marker = self.marker([peak(100, 20), peak(104, 100)], kit)
        self.assertEqual(len(marker['calls']), 2)

    def test_forward_stutter_uses_later_height_over_earlier_height(self):
        kit = kit_definition(forward_stutter_ranges=[{'min_bp': 4, 'max_bp': 4, 'ratio': 0.2}])
        marker = self.marker([peak(100, 100), peak(104, 19)], kit)
        self.assertEqual([c['size_bp'] for c in marker['calls']], [100])
        self.assertEqual(marker['peak_review'][1]['state'], 'forward_stutter')
        equality = self.marker([peak(100, 100), peak(104, 20)], kit)
        self.assertEqual(len(equality['calls']), 2)

    def test_backward_mutation_precedes_forward_and_removed_parents_are_unavailable(self):
        kit = kit_definition(backward_stutter_ranges=[{'min_bp': 4, 'max_bp': 4, 'ratio': 0.2}],
                             forward_stutter_ranges=[{'min_bp': 8, 'max_bp': 8, 'ratio': 0.2}])
        marker = self.marker([peak(100, 10), peak(104, 100), peak(108, 1)], kit)
        # 108 would be forward-stutter relative to 100 if its removed parent
        # were compared independently against the original eligibility mask.
        self.assertEqual([c['size_bp'] for c in marker['calls']], [104, 108])
        self.assertEqual([c['size_bp'] for c in marker['stutter_calls']], [100])

    def test_explicit_empty_stutter_ranges_disable_both_directions(self):
        marker = self.marker([peak(100, 1), peak(104, 100), peak(108, 1)])
        self.assertEqual(len(marker['calls']), 3)
        self.assertEqual(marker['stutter_calls'], [])

    def test_bin_edges_are_inclusive_and_offset_is_added_without_changing_size(self):
        kit = kit_definition()
        for size in (98.5, 99.5):
            marker = self.marker([peak(size)], kit, ladder=offsets(kit, delta=1))
            self.assertEqual(marker['calls'][0]['label'], '1')
            self.assertEqual(marker['calls'][0]['size_bp'], size)
            self.assertFalse(marker['calls'][0]['off_ladder'])
        marker = self.marker([peak(100)], kit, ladder=offsets(kit, delta=1))
        self.assertEqual(marker['calls'][0]['label'], 'OL')

    def test_per_bin_offsets_are_used_independently(self):
        kit = kit_definition()
        ladder = offsets(kit)
        ladder['markers']['SyntheticM']['bin_offsets']['2'] = 2
        marker = self.marker([peak(102)], kit, ladder=ladder)
        self.assertEqual(marker['calls'][0]['label'], '2')

    def test_gap_stays_off_ladder_without_nearest_bin_assignment(self):
        marker = self.marker([peak(102)])
        self.assertEqual(marker['calls'][0]['label'], 'OL')
        self.assertTrue(marker['calls'][0]['off_ladder'])
        self.assertEqual(marker['calls'][0]['bin_hits'], [])
        self.assertIn('no_bin_hit', marker['peak_review'][0]['reasons'])

    def test_touching_bins_are_ambiguous_even_at_exact_shared_boundary(self):
        kit = kit_definition()
        kit['markers'][0]['bins'][0]['right_bp'] = 2
        kit['markers'][0]['bins'][1]['left_bp'] = 2
        marker = self.marker([peak(102)], kit)
        call = marker['calls'][0]
        self.assertEqual(call['label'], 'OL')
        self.assertEqual(call['bin_hits'], ['1', '2'])
        self.assertTrue(call['off_ladder'])
        self.assertIn('multiple_bin_hits', marker['peak_review'][0]['reasons'])

    def test_stutter_label_does_not_receive_selected_off_ladder_or_weight_fields(self):
        kit = kit_definition(backward_stutter_ranges=[{'min_bp': 4, 'max_bp': 4, 'ratio': 0.2}])
        kit['markers'][0]['bins'][1]['confidence_weight'] = 0.75
        marker = self.marker([peak(100, 10), peak(104, 100)], kit)
        self.assertEqual(marker['calls'][0]['confidence_weight'], 0.75)
        stutter = marker['stutter_calls'][0]
        self.assertEqual(stutter['label'], '1')
        self.assertNotIn('off_ladder', stutter)
        self.assertNotIn('confidence_weight', stutter)

    def test_explicit_artifact_flags_exclude_before_cutoff_but_remain_reviewable(self):
        peaks = [peak(100, 10000, spike=True), peak(104, 10000, flags={'pullup': True}),
                 peak(108, 10, flags=['unreviewed'])]
        marker = self.marker(peaks, kit_definition(cutoff_ratio=0.5))
        self.assertEqual([c['size_bp'] for c in marker['calls']], [108])
        self.assertEqual(marker['cutoff_height'], 5)
        self.assertEqual([r['state'] for r in marker['peak_review']],
                         ['excluded_artifact', 'excluded_artifact', 'selected'])
        self.assertEqual(marker['peak_review'][0]['reasons'], ['spike'])
        self.assertEqual(marker['peak_review'][1]['reasons'], ['pullup'])

    def test_clear_range_removes_selected_and_stutter_calls_inclusive(self):
        kit = kit_definition(backward_stutter_ranges=[{'min_bp': 4, 'max_bp': 4, 'ratio': 0.2}],
                             clear_ranges=[{'min_bp': 100, 'max_bp': 104}])
        marker = self.marker([peak(100, 10), peak(104, 100), peak(108, 100)], kit)
        self.assertEqual([c['size_bp'] for c in marker['calls']], [108])
        self.assertEqual(marker['stutter_calls'], [])
        self.assertEqual(marker['peak_review'][0]['reasons'], ['backward_stutter', 'clear_range'])
        self.assertEqual(marker['peak_review'][1]['state'], 'clear_range')

    def test_stutter_fraction_annotation_is_separate_strict_open_distance_rule(self):
        marker = self.marker([peak(100, 19), peak(104, 100)])
        self.assertEqual(marker['calls'][1]['stutter_fraction_annotation'], 0.19)
        self.assertEqual(len(marker['calls']), 2)
        for position, height in ((99.5, 19), (100.5, 19), (100, 20)):
            marker = self.marker([peak(position, height), peak(104, 100)])
            self.assertIsNone(marker['calls'][1]['stutter_fraction_annotation'])

    def test_window_subtracts_offsets_and_keeps_endpoints(self):
        kit = kit_definition()
        marker = self.marker([peak(89), peak(129), peak(130)], kit, ladder=offsets(kit, delta=1))
        self.assertEqual(marker['window_bp'], [89, 129])
        self.assertEqual([r['size_bp'] for r in marker['peak_review']], [89, 129])
        result = self.call([peak(89), peak(129), peak(130)], kit, offsets(kit, 1))
        self.assertEqual(result['unassigned_peak_indices'], [2])

    def test_channel_routing_ignores_other_channel(self):
        result = self.call([peak(100), peak(104, channel='DATA2')])
        self.assertEqual([c['size_bp'] for c in result['markers'][0]['calls']], [100])
        self.assertEqual(result['unassigned_peak_indices'], [1])

    def test_missing_or_bad_sizing_never_emits_calls(self):
        for accepted in (False, None, 1, 'accepted'):
            result = self.call([peak(100)], sizing_accepted=accepted)
            self.assertEqual(result['status'], 'rejected')
            self.assertEqual(result['markers'], [])
        for value in (math.nan, math.inf, True, None):
            result = self.call([peak(100, size_bp=value)])
            self.assertEqual(result['status'], 'rejected')
            self.assertEqual(result['markers'], [])

    def test_missing_unaccepted_and_nonfinite_offsets_reject_without_silent_zero(self):
        kit = kit_definition()
        variants = [{}, {'markers': {}}, offsets(kit), offsets(kit), offsets(kit), offsets(kit)]
        variants[2]['markers']['SyntheticM']['status'] = 'tentative'
        del variants[3]['markers']['SyntheticM']['bin_offsets']['4']
        variants[4]['markers']['SyntheticM']['bin_offsets']['1'] = math.nan
        variants[5]['markers']['SyntheticM']['min_offset_bp'] = None
        for ladder in variants:
            result = self.call([peak(100)], kit, ladder)
            self.assertEqual(result['status'], 'rejected', ladder)
            self.assertEqual(result['markers'][0]['calls'], [])
            self.assertEqual(result['markers'][0]['stutter_calls'], [])

    def test_cross_marker_overlap_uses_peak_identity_not_matching_label(self):
        kit = kit_definition()
        second = deepcopy(kit['markers'][0])
        second['name'] = 'SyntheticOther'
        kit['markers'].append(second)
        result = self.call([peak(100)], kit)
        for marker in result['markers']:
            self.assertIn('cross_marker_overlap', marker['calls'][0]['flags'])
            self.assertEqual(marker['events'][-1]['reason'], 'cross_marker_overlap')
        second['channel'] = 'DATA2'
        result = self.call([peak(100), peak(100, channel='DATA2')], kit)
        for marker in result['markers']:
            self.assertNotIn('cross_marker_overlap', marker['calls'][0]['flags'])

    def test_partial_ladder_does_not_block_other_explicitly_accepted_marker(self):
        kit = kit_definition()
        second = deepcopy(kit['markers'][0])
        second['name'] = 'RejectedM'
        kit['markers'].append(second)
        ladder = offsets(kit)
        ladder['markers']['RejectedM']['status'] = 'rejected'
        result = self.call([peak(100)], kit, ladder)
        self.assertEqual(result['status'], 'partial')
        self.assertEqual(len(result['markers'][0]['calls']), 1)
        self.assertEqual(result['markers'][1]['calls'], [])

    def test_input_and_pair_work_are_bounded(self):
        for peaks in ([peak(100)] * 4097, [peak(100)] * 513):
            result = self.call(peaks)
            self.assertEqual(result['status'], 'rejected')
        kit = kit_definition(backward_stutter_ranges=[{'min_bp': 4, 'max_bp': 4, 'ratio': 0}] * 64)
        self.assertEqual(self.call([peak(100)] * 512, kit)['status'], 'rejected')

    def test_malformed_peak_flags_and_kit_have_explicit_rejection(self):
        for fields in ({'height': 0}, {'height': -1}, {'scan': -1},
                       {'flags': 'spike'}, {'spike': 1}, {'flags': {'spike': 'yes'}}):
            result = self.call([peak(100, **fields)])
            self.assertEqual(result['status'], 'rejected', fields)
            self.assertTrue(result['reasons'])
        kit = kit_definition()
        del kit['markers'][0]['parameters']['cutoff_ratio']
        self.assertEqual(self.call([peak(100)], kit)['status'], 'rejected')

    def test_empty_sample_is_tentative_with_no_forced_alleles(self):
        marker = self.marker([])
        self.assertEqual(marker['calls'], [])
        self.assertEqual(marker['stutter_calls'], [])
        self.assertEqual(marker['peak_review'], [])


if __name__ == '__main__':
    unittest.main()
