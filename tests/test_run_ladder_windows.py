from copy import deepcopy
import json
import math
import unittest

from genemapper_open.ladder import match_ladder
from genemapper_open.run_offsets import (
    ladder_window_offset, upsert_run_offsets, remove_run_ladder, store_offset_bp,
)
from genemapper_open.str_demo import make_str_demo


def fixture():
    prototype = make_str_demo()['kit']['markers'][0]
    markers = []
    for name, low, high, centers in (
        ('First', 160, 180, (168, 176)),
        ('Second', 188, 212, (192, 200)),
        ('Third', 220, 244, (224, 232)),
    ):
        marker = deepcopy(prototype)
        marker.update(name=name, min_bp=low, max_bp=high, ladder_labels=['A', 'B'],
                      bins=[{'label': label, 'center_bp': bp, 'left_bp': .5, 'right_bp': .5}
                            for label, bp in zip(('A', 'B'), centers)])
        markers.append(marker)
    kit = {'schema_version': 1, 'name': 'Explicit run ladder geometry', 'markers': markers}
    peaks = [{'channel': 'DATA1', 'scan': 200+20*(bp-.3), 'size_bp': bp-.3, 'height': 1000}
             for bp in (168, 176, 192, 200, 224, 232)]
    return peaks, kit


def contribution(state, identifier, marker, offset, code=0):
    return upsert_run_offsets(state, identifier, code, {'status': 'partial', 'markers': {
        marker: {'status': 'accepted', 'bin_offsets': {}, 'max_offset_bp': offset},
    }})


def match(peaks, kit, **extra):
    return match_ladder(peaks, kit, size_standard_max_bp=260, sizing_accepted=True, **extra)


class RunLadderWindowTests(unittest.TestCase):
    def test_coupled_local_offset_uses_nine_decimal_storage_only_in_run_path(self):
        peaks, kit = fixture()
        single = match(peaks, kit)
        coupled = match(peaks, kit, run_offset_state={'schema_version': 1, 'contributions': [], 'events': []})
        raw = single['markers']['First']['max_offset_bp']
        stored = store_offset_bp(raw)
        self.assertNotEqual(raw, stored)
        self.assertEqual(coupled['markers']['First']['max_offset_bp'], raw)
        self.assertEqual(single['markers']['Second']['diagnostics']['previous_virtual_max_offset_bp'], raw)
        self.assertEqual(coupled['markers']['Second']['diagnostics']['previous_virtual_max_offset_bp'], stored)
        self.assertEqual(coupled['markers']['Second']['diagnostics']['previous_offset_sources']['local_offset_bp'], stored)

    def test_coupled_local_negative_tie_and_sequential_mean_match_storage_contract(self):
        self.assertEqual(ladder_window_offset(None, 'First', -1.5e-9)['local_offset_bp'], -1e-9)
        state = contribution(None, 'one.fsa', 'First', 1_000_000.)
        state = contribution(state, 'two.fsa', 'First', 1e-9)
        actual = ladder_window_offset(state, 'First', -1_000_000.)['offset_bp']
        expected = ((1_000_000.+1e-9)-1_000_000.)/3
        self.assertEqual(actual, expected)
        self.assertNotEqual(actual, math.fsum([1_000_000., 1e-9, -1_000_000.])/3)

    def test_default_and_explicit_none_preserve_single_ladder_json(self):
        peaks, kit = fixture()
        default = match(peaks, kit)
        self.assertEqual(json.dumps(default, sort_keys=True),
                         json.dumps(match(peaks, kit, run_offset_state=None), sort_keys=True))
        self.assertNotIn('run_window_coupling', default['diagnostics'])
        self.assertNotIn('previous_offset_sources', default['markers']['Second']['diagnostics'])

    def test_raw_run_values_and_local_value_are_weighted_individually(self):
        peaks, kit = fixture()
        state = contribution(None, 'one.fsa', 'First', 1)
        state = contribution(state, 'two.fsa', 'First', 3)
        before = deepcopy((state, peaks, kit))
        called = match(peaks, kit, run_offset_state=state)
        local = called['markers']['First']['max_offset_bp']
        expected = (1+3+local)/3
        second = called['markers']['Second']
        self.assertAlmostEqual(second['window']['min_bp'], 180-expected)
        self.assertNotAlmostEqual(expected, (2+local)/2)
        sources = second['diagnostics']['previous_offset_sources']
        self.assertEqual(sources['marker'], 'First')
        self.assertEqual(sources['value_count'], 3)
        self.assertEqual(len(sources['run_contributors']), 2)
        self.assertFalse(sources['local_quality_gate_applied'])
        self.assertEqual((state, peaks, kit), before)

    def test_first_marker_ignores_own_run_offset_and_later_window_uses_previous_key(self):
        peaks, kit = fixture()
        state = contribution(None, 'one.fsa', 'First', 1)
        state = contribution(state, 'one.fsa', 'Second', 17)
        result = match(peaks, kit, run_offset_state=state)
        self.assertEqual(result['markers']['First']['window']['min_bp'], 153)
        self.assertEqual(result['markers']['First']['window']['max_bp'], 188)
        local_first = result['markers']['First']['max_offset_bp']
        self.assertAlmostEqual(result['markers']['Second']['window']['min_bp'], 180-(1+local_first)/2)
        local_second = result['markers']['Second']['max_offset_bp']
        self.assertAlmostEqual(result['markers']['Third']['window']['min_bp'], 212-(17+local_second)/2)

    def test_upper_geometry_subtracts_same_previous_offset_before_next_minimum(self):
        peaks, kit = fixture()
        kit['markers'][1]['max_bp'] = 240
        state = contribution(None, 'one.fsa', 'First', 1)
        result = match(peaks, kit, run_offset_state=state)
        previous = (1+result['markers']['First']['max_offset_bp'])/2
        self.assertAlmostEqual(result['markers']['Second']['window']['max_bp'], 240-previous)
        self.assertEqual(result['markers']['Third']['window']['max_bp'], 260)

    def test_rejected_local_previous_marker_still_uses_admitted_run_values(self):
        peaks, kit = fixture()
        peaks = [p for p in peaks if p['size_bp'] > 180]
        state = contribution(None, 'one.fsa', 'First', 1)
        state = contribution(state, 'two.fsa', 'First', 3)
        result = match(peaks, kit, run_offset_state=state)
        self.assertEqual(result['markers']['First']['status'], 'rejected')
        second = result['markers']['Second']
        self.assertEqual(second['window']['min_bp'], 178)
        self.assertIsNone(second['diagnostics']['previous_offset_sources']['local_offset_bp'])

    def test_only_admitted_codes_enter_window_mean_and_empty_is_zero(self):
        state = None
        for i, (value, code) in enumerate(((1, 0), (2, -2), (3, .5), (99, None), (99, 1), (99, 2))):
            state = contribution(state, str(i)+'.fsa', 'First', value, code)
        offset = ladder_window_offset(state, 'First', 4)
        self.assertEqual(offset['offset_bp'], 2.5)
        self.assertEqual(offset['value_count'], 4)
        self.assertEqual(ladder_window_offset(None, 'First')['offset_bp'], 0)
        self.assertEqual(ladder_window_offset(None, 'First', -3)['offset_bp'], -3)

    def test_current_filename_is_explicitly_withdrawn_before_matching(self):
        peaks, kit = fixture()
        state = contribution(None, 'CURRENT.FSA', 'First', 99)
        state = contribution(state, 'other.fsa', 'First', 1)
        state = remove_run_ladder(state, 'current.fsa')
        result = match(peaks, kit, run_offset_state=state)
        sources = result['markers']['Second']['diagnostics']['previous_offset_sources']
        self.assertEqual([r['ladder_id'] for r in sources['run_contributors']], ['other.fsa'])

    def test_invalid_run_state_returns_rejected_diagnostics(self):
        peaks, kit = fixture()
        result = match(peaks, kit, run_offset_state={'schema_version': 1})
        self.assertEqual(result['status'], 'rejected')
        self.assertIn('state', result['reasons'][0])


if __name__ == '__main__':
    unittest.main()
