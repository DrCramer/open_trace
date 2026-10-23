"""Synthetic ladder shifts, exhaustive objectives and explicit review gates."""
import copy
import itertools
import json
import random
import unittest

from genemapper_open.ladder import match_ladder


def kit_fixture(centers=(100, 104, 108), **parameters):
    params = {'ladder_cutoff_ratio': 0, 'cutoff_ratio': .2,
              'minus_a_ratio': 0, 'minus_a_min_bp': 0, 'minus_a_max_bp': 0,
              'backward_stutter_ranges': [], 'forward_stutter_ranges': []}
    params.update(parameters)
    return {'schema_version': 1, 'name': 'Synthetic kit', 'markers': [{
        'name': 'M', 'channel': 'DATA1', 'min_bp': min(centers)-2,
        'max_bp': max(centers)+2, 'repeat': 4,
        'bins': [{'label': str(i), 'center_bp': c, 'left_bp': .5, 'right_bp': .5}
                 for i, c in enumerate(centers, 1)],
        'ladder_labels': [str(i) for i in range(1, len(centers)+1)], 'parameters': params}]}


def peaks(sizes, heights=None):
    return [{'scan': 1000+100*i, 'size_bp': bp, 'height': height}
            for i, (bp, height) in enumerate(zip(sizes, heights if heights is not None else [100]*len(sizes)))]


def exhaustive(sizes, heights, centers):
    options = []
    for path in itertools.combinations(range(len(sizes)), len(centers)):
        geometry = 0
        for index in range(2, len(path)):
            a, b, c = (sizes[path[j]] for j in (index-2, index-1, index))
            observed = (c-b)/(b-a)
            expected = (centers[index]-centers[index-1])/(centers[index-1]-centers[index-2])
            geometry += min(observed/expected, expected/observed)
        score = (.4*sum(heights[i]/max(heights) for i in path)+.6*geometry)/(len(centers)-1.2)
        options.append((score, path))
    return sorted(options, key=lambda row: (-row[0], row[1]))


class LadderTests(unittest.TestCase):
    def match(self, rows, kit=None, **kwargs):
        return match_ladder({'DATA1': rows}, kit or kit_fixture(),
                            size_standard_max_bp=kwargs.pop('size_standard_max_bp', 1000),
                            sizing_accepted=kwargs.pop('sizing_accepted', True), **kwargs)

    def test_shifted_ladder_offsets_and_immutable_json_output(self):
        rows = peaks([98, 102, 106])
        before = copy.deepcopy(rows)
        result = self.match(rows)
        self.assertEqual(result['status'], 'accepted')
        marker = result['markers']['M']
        self.assertEqual(marker['bin_offsets'], {'1': 2, '2': 2, '3': 2})
        self.assertEqual(marker['min_offset_bp'], 2)
        self.assertEqual(marker['max_offset_bp'], 2)
        self.assertAlmostEqual(marker['confidence'], 1)
        self.assertEqual(rows, before)
        json.dumps(result, allow_nan=False)

    def test_generated_affine_size_distortions_have_expected_offsets(self):
        rng = random.Random(616)
        for length in (2, 3, 5, 8):
            centers = sorted(rng.sample(range(100, 400), length))
            for scale, shift in ((1, -2.4), (1.001, -1.3), (.998, 1.1)):
                sizes = [center*scale+shift for center in centers]
                result = self.match(peaks(sizes), kit_fixture(centers))
                with self.subTest(length=length, scale=scale, shift=shift):
                    self.assertEqual(result['status'], 'accepted')
                    for center, row in zip(centers, result['markers']['M']['matched']):
                        self.assertAlmostEqual(row['offset_bp'], center-(center*scale+shift))

    def test_raw_height_objective_matches_exhaustive_search(self):
        rng = random.Random(612)
        for length, count in ((2, 5), (3, 6), (4, 7)):
            for _ in range(8):
                centers = [100+10*i*i for i in range(length)]
                sizes = sorted(rng.sample(range(92, 260), count))
                heights = [rng.randrange(10, 100) for _ in sizes]
                result = self.match(peaks(sizes, heights), kit_fixture(centers), ambiguity_margin=0)
                diagnostic = result['markers']['M']['diagnostics']
                # Top-2N trimming is an explicit stage before raw-height scoring.
                indices = diagnostic['candidate_indices']
                expected = exhaustive([sizes[i] for i in indices], [heights[i] for i in indices], centers)
                self.assertAlmostEqual(diagnostic['best_score'], expected[0][0], places=12)
                self.assertAlmostEqual(diagnostic['second_best_score'], expected[1][0], places=12)
                actual = tuple(row['candidate_index'] for row in diagnostic['proposed_matches'])
                self.assertEqual(actual, tuple(indices[i] for i in expected[0][1]))

    def test_ladder_cutoff_keeps_equality(self):
        kit = kit_fixture(ladder_cutoff_ratio=.5)
        self.assertEqual(self.match(peaks([100, 104, 108], [100, 50, 100]), kit)['status'], 'accepted')
        below = self.match(peaks([100, 104, 108], [100, 49.999, 100]), kit)
        self.assertEqual(below['status'], 'rejected')
        self.assertEqual(below['markers']['M']['bin_offsets'], {})

    def test_window_is_inclusive_and_last_upper_comes_from_standard(self):
        kit = kit_fixture((100, 104))
        kit['markers'][0]['min_bp'] = 98
        result = self.match(peaks([91, 95]), kit, size_standard_max_bp=95)
        self.assertEqual(result['status'], 'accepted')
        self.assertEqual(result['markers']['M']['diagnostics']['window_count'], 2)
        self.assertEqual(self.match(peaks([90.999, 95]), kit, size_standard_max_bp=95)['status'], 'rejected')

    def test_explicit_spike_pullup_excluded_offscale_retained(self):
        rows = peaks([96, 98, 100, 104, 108], [1e6, 1e6, 100, 100, 100])
        rows[0]['flags'] = {'spike': True}
        rows[1]['pullup'] = True
        rows[2]['flags'] = ['offscale']
        result = self.match(rows)
        self.assertEqual(result['status'], 'accepted')
        self.assertEqual(result['markers']['M']['diagnostics']['flag_excluded_count'], 2)
        self.assertEqual(result['markers']['M']['bin_offsets'], {'1': 0, '2': 0, '3': 0})

    def test_top_2n_keeps_ties_but_enforces_computation_bound(self):
        rows = peaks([92+i for i in range(9)], [1, 2, 3, 3, 3, 3, 3, 3, 4])
        result = self.match(rows)
        diagnostic = result['markers']['M']['diagnostics']
        self.assertEqual(diagnostic['top_2n_height'], 3)
        self.assertEqual(diagnostic['selected_count'], 7)
        tied = self.match(peaks([92+i*.1 for i in range(65)]))
        self.assertIn('64-candidate', tied['markers']['M']['reasons'][0])
        self.assertEqual(tied['markers']['M']['bin_offsets'], {})

    def test_equal_assignments_and_explicit_ambiguity_margin_withhold_offsets(self):
        tied = self.match(peaks([100, 104, 108, 112]))
        self.assertEqual(tied['status'], 'rejected')
        self.assertEqual(tied['markers']['M']['bin_offsets'], {})
        rows = peaks([100, 101, 104, 108], [100, 50, 100, 100])
        accepted = self.match(rows, ambiguity_margin=0)
        self.assertEqual(accepted['status'], 'accepted')
        gap = accepted['markers']['M']['diagnostics']['ambiguity_gap']
        self.assertEqual(self.match(rows, ambiguity_margin=gap)['status'], 'rejected')

    def test_confidence_penalty_uses_strict_unmatched_threshold(self):
        for extra_height, count in ((100, 0), (100.1, 1)):
            result = self.match(peaks([100, 101, 104, 108], [100, extra_height, 100, 100]), ambiguity_margin=0)
            marker = result['markers']['M']
            with self.subTest(height=extra_height):
                self.assertEqual(marker['status'], 'accepted')
                self.assertEqual(marker['diagnostics']['unmatched_height_threshold'], 100)
                self.assertEqual(marker['diagnostics']['penalty_unmatched_count'], count)
                self.assertAlmostEqual(marker['confidence'], marker['diagnostics']['best_score']*(1-.25*count))

    def test_confidence_uses_sample_standard_deviation(self):
        result = self.match(peaks([100, 104, 105, 108], [90, 100, 88, 110]), ambiguity_margin=0)
        marker = result['markers']['M']
        self.assertEqual(marker['status'], 'accepted')
        self.assertAlmostEqual(marker['diagnostics']['unmatched_height_threshold'], 90)
        self.assertEqual(marker['diagnostics']['penalty_unmatched_count'], 0)

    def test_sizing_gate_invalid_flags_and_unsized_values(self):
        for accepted in (False, 1, 'accepted', None):
            result = self.match(peaks([100, 104, 108]), sizing_accepted=accepted)
            self.assertEqual(result['status'], 'rejected')
            self.assertEqual(result['markers'], {})
        for key, value in (('size_bp', None), ('height', float('nan')), ('spike', 'yes')):
            rows = peaks([100, 104, 108]); rows[0][key] = value
            self.assertEqual(self.match(rows)['status'], 'rejected')

    def test_no_migration_rate_gate_and_no_partial_ladder(self):
        # bp/bp slope1 would fail the size-standard scan-rate bound .25.
        self.assertEqual(self.match(peaks([100, 104, 108]))['status'], 'accepted')
        short = self.match(peaks([100, 104]))
        self.assertEqual(short['markers']['M']['matched'], [])
        self.assertEqual(short['markers']['M']['bin_offsets'], {})

    def test_low_scoring_full_assignment_withholds_offsets(self):
        result = self.match(peaks([100, 101, 150]), kit_fixture((100, 104, 108)))
        marker = result['markers']['M']
        self.assertLess(marker['diagnostics']['best_score'], .75)
        self.assertTrue(marker['diagnostics']['complete'])
        self.assertEqual(marker['status'], 'rejected')
        self.assertEqual(marker['matched'], [])
        self.assertEqual(marker['bin_offsets'], {})

    def test_missing_channel_is_a_rejected_marker_and_other_offsets_remain_separate(self):
        kit = kit_fixture()
        other = copy.deepcopy(kit['markers'][0])
        other.update(name='N', channel='DATA2')
        kit['markers'].append(other)
        result = self.match(peaks([100, 104, 108]), kit)
        self.assertEqual(result['status'], 'partial')
        self.assertEqual(result['markers']['M']['status'], 'accepted')
        self.assertEqual(result['markers']['N']['status'], 'rejected')
        self.assertEqual(result['markers']['N']['bin_offsets'], {})

    def test_clear_range_marks_rows_but_retains_offsets(self):
        kit = kit_fixture(clear_ranges=[{'min_bp': 104, 'max_bp': 104}])
        marker = self.match(peaks([100, 104, 108]), kit)['markers']['M']
        self.assertEqual([row['cleared'] for row in marker['matched']], [False, True, False])
        self.assertEqual(len(marker['bin_offsets']), 3)

    def test_previous_marker_offset_controls_next_window(self):
        kit = kit_fixture()
        second = copy.deepcopy(kit_fixture((120, 124, 128))['markers'][0]); second['name'] = 'N'
        kit['markers'].append(second)
        result = self.match(peaks([98, 102, 106, 118, 122, 126]), kit)
        self.assertEqual(result['status'], 'accepted')
        self.assertEqual(result['markers']['N']['window']['min_bp'], 108)
        self.assertEqual(result['markers']['N']['diagnostics']['previous_virtual_max_offset_bp'], 2)

    def test_flat_peaks_and_missing_or_unknown_ladder_definitions(self):
        rows = [dict(row, channel='DATA1') for row in peaks([100, 104, 108])]
        result = match_ladder(rows, kit_fixture(), size_standard_max_bp=500, sizing_accepted=True)
        self.assertEqual(result['status'], 'accepted')
        for labels in ([], ['1']):
            kit = kit_fixture(); kit['markers'][0]['ladder_labels'] = labels
            self.assertEqual(self.match(peaks([100, 104, 108]), kit)['status'], 'rejected')


if __name__ == '__main__':
    unittest.main()
