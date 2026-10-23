from copy import deepcopy
import json
import math
import unittest

from genemapper_open.run_offsets import (
    aggregate_run_offsets, upsert_run_offsets, remove_run_ladder, store_offset_bp,
)
from genemapper_open.str_demo import make_str_demo
from genemapper_open.allele_calling import call_sample


def kit():
    return make_str_demo()['kit']


def result(offsets=None, low=None, high=None, *, status='accepted', marker_status='accepted'):
    return {'status': status, 'markers': {'ExampleM': {
        'status': marker_status, 'bin_offsets': offsets or {},
        'min_offset_bp': low, 'max_offset_bp': high,
    }}}


def entry(identifier, source, code=0):
    return {'ladder_id': identifier, 'quality_code': code, 'result': source}


class RunOffsetTests(unittest.TestCase):
    def test_nine_decimal_storage_has_java_positive_infinity_ties(self):
        for value, expected in ((.5e-9, 1e-9), (1.5e-9, 2e-9), (2.5e-9, 3e-9),
                                (-.5e-9, 0.), (-1.5e-9, -1e-9), (-2.5e-9, -2e-9),
                                (1_000_000., 1_000_000.), (-1_000_000., -1_000_000.)):
            with self.subTest(value=value):
                self.assertEqual(store_offset_bp(value), expected)
        self.assertEqual(math.copysign(1, store_offset_bp(-.5e-9)), 1)

    def test_storage_rounds_binary_product_without_a_floating_half_addition(self):
        below_half = math.nextafter(.5e-9, 0.)
        self.assertEqual(below_half*1e9, math.nextafter(.5, 0.))
        self.assertEqual(math.floor(below_half*1e9+.5), 1)  # double-addition artifact
        self.assertEqual(store_offset_bp(below_half), 0.)
        self.assertEqual(store_offset_bp(math.nextafter(-.5e-9, -math.inf)), -1e-9)

    def test_upsert_stores_bins_and_each_endpoint_before_aggregation(self):
        source = result({'A': 1.5e-9, 'B': -1.5e-9}, .5e-9, -.5e-9)
        state = upsert_run_offsets(None, 'own.fsa', 0, source)
        by_key = {(r['key_kind'], r['bin_label']): r['offset_bp'] for r in state['contributions']}
        self.assertEqual(by_key, {('bin', 'A'): 2e-9, ('bin', 'B'): -1e-9,
                                  ('virtual_min', None): 1e-9, ('virtual_max', None): 0.})
        marker = aggregate_run_offsets([entry('own.fsa', source)], kit())['markers']['ExampleM']
        self.assertEqual(marker['bin_offsets'], {'A': 2e-9, 'B': -1e-9})
        self.assertEqual(marker['min_offset_bp'], 1e-9)
        self.assertEqual(marker['max_offset_bp'], 0.)

    def test_mean_uses_sequential_double_addition_in_supplied_order(self):
        values = [1_000_000., 1e-9, -1_000_000.]
        entries = [entry(str(i)+'.fsa', result({'A': value}, value, value))
                   for i, value in enumerate(values)]
        marker = aggregate_run_offsets(entries, kit())['markers']['ExampleM']
        expected = ((values[0]+values[1])+values[2])/3
        self.assertEqual(marker['bin_offsets']['A'], expected)
        self.assertEqual(marker['min_offset_bp'], expected)
        self.assertEqual(marker['max_offset_bp'], expected)
        self.assertNotEqual(expected, math.fsum(values)/3)

    def test_means_are_per_key_with_independent_endpoint_denominators(self):
        aggregated = aggregate_run_offsets([
            entry('first.fsa', result({'A': 1, 'B': 2}, 10, 20)),
            entry('second.fsa', result({'A': 3, 'C': 6}, 30)),
        ], kit())
        marker = aggregated['markers']['ExampleM']
        self.assertEqual(marker['bin_offsets'], {'A': 2, 'B': 2, 'C': 6})
        self.assertEqual(marker['min_offset_bp'], 20)
        self.assertEqual(marker['max_offset_bp'], 20)
        self.assertEqual(marker['missing_bin_offsets'], ['D'])
        self.assertEqual(len(marker['per_key']['bins']['A']['contributors']), 2)
        self.assertEqual(len(marker['per_key']['bins']['B']['contributors']), 1)
        self.assertEqual(aggregated['status'], 'accepted')

    def test_only_exact_explicit_quality_codes_are_admitted(self):
        entries = [entry('ladder-'+str(i), result({'A': i+1}, i+1, i+1), code)
                   for i, code in enumerate((0, -2, .5, None, 1, 2, -1, 1e-14, .5+1e-15))]
        marker = aggregate_run_offsets(entries, kit())['markers']['ExampleM']
        self.assertEqual(marker['bin_offsets']['A'], 2)
        diagnostics = marker['per_key']['bins']['A']
        self.assertEqual([r['quality_code'] for r in diagnostics['contributors']], [0, -2, .5])
        self.assertEqual(diagnostics['excluded'][0]['reason'], 'quality_code_missing')
        self.assertEqual(len(diagnostics['excluded']), 6)

    def test_fresh_case_insensitive_reanalysis_replaces_all_old_keys(self):
        aggregated = aggregate_run_offsets([
            entry('LADDER.FSA', result({'A': 1, 'B': 2}, 3, 4)),
            entry('ladder.fsa', result({'A': 10}, 20)),
        ], kit())
        marker = aggregated['markers']['ExampleM']
        self.assertEqual(marker['bin_offsets'], {'A': 10})
        self.assertEqual(marker['min_offset_bp'], 20)
        self.assertIsNone(marker['max_offset_bp'])
        history = aggregated['diagnostics']['entry_history'][1]
        self.assertEqual(history['replaced_ladder_id'], 'LADDER.FSA')
        self.assertTrue(history['case_insensitive_replacement'])
        self.assertEqual(history['withdrawn_contributions'], 4)
        self.assertEqual(marker['source_ladders'], ['ladder.fsa'])

    def test_repeated_same_filename_has_one_vote_per_key(self):
        marker = aggregate_run_offsets([
            entry('same.fsa', result({'A': 1}, 1, 1)),
            entry('other.fsa', result({'A': 7}, 7, 7)),
            entry('same.fsa', result({'A': 3}, 3, 3)),
        ], kit())['markers']['ExampleM']
        self.assertEqual(marker['bin_offsets']['A'], 5)
        self.assertEqual(len(marker['per_key']['bins']['A']['contributors']), 2)

    def test_rejected_empty_or_unavailable_reanalysis_withdraws_old_admitted_values(self):
        for replacement, code in (
            ({'status': 'rejected', 'markers': {}}, 0),
            (result(), 0),
            (result({'A': 9}, 9, 9), None),
            (result({'A': 9}, 9, 9), 2),
        ):
            with self.subTest(replacement=replacement, code=code):
                aggregated = aggregate_run_offsets([
                    entry('same.fsa', result({'A': 1}, 1, 1)),
                    entry('SAME.FSA', replacement, code),
                ], kit())
                marker = aggregated['markers']['ExampleM']
                self.assertEqual(marker['bin_offsets'], {})
                self.assertIsNone(marker['min_offset_bp'])
                self.assertEqual(marker['source_ladders'], [])
                self.assertEqual(aggregated['status'], 'rejected')

    def test_standalone_upsert_preserves_absent_keys_and_updates_only_same_key_identity(self):
        state = upsert_run_offsets(None, 'FIRST.FSA', 0, result({'A': 1, 'B': 2}, 3, 4))
        before = deepcopy(state)
        updated = upsert_run_offsets(state, 'first.fsa', .5, result({'A': 10}, 20))
        self.assertEqual(state, before)
        by_key = {(r['key_kind'], r['bin_label']): r for r in updated['contributions']}
        self.assertEqual(len(by_key), 4)
        self.assertEqual(by_key[('bin', 'A')]['offset_bp'], 10)
        self.assertEqual(by_key[('bin', 'A')]['quality_code'], .5)
        self.assertEqual(by_key[('bin', 'B')]['offset_bp'], 2)
        self.assertEqual(by_key[('bin', 'B')]['quality_code'], 0)
        self.assertEqual(by_key[('virtual_max', None)]['offset_bp'], 4)
        self.assertEqual(updated['events'][-1]['replaced_keys'], 2)

    def test_explicit_remove_withdraws_all_keys_for_one_filename_only(self):
        state = upsert_run_offsets(None, 'first.fsa', 0, result({'A': 1, 'B': 2}, 3, 4))
        state = upsert_run_offsets(state, 'second.fsa', 0, result({'A': 5}, 6, 7))
        before = deepcopy(state)
        removed = remove_run_ladder(state, 'FIRST.FSA')
        self.assertEqual(state, before)
        self.assertEqual({r['ladder_id'] for r in removed['contributions']}, {'second.fsa'})
        self.assertEqual(removed['events'][-1]['removed_contributions'], 4)

    def test_structured_keys_do_not_collapse_bin_label_and_virtual_endpoint(self):
        source = result({'A': 1, 'VirtualBinMarkerSizeRangeMin': 9}, 2, 3)
        state = upsert_run_offsets(None, 'first.fsa', 0, source)
        keys = [(r['key_kind'], r['bin_label']) for r in state['contributions']]
        self.assertIn(('bin', 'VirtualBinMarkerSizeRangeMin'), keys)
        self.assertIn(('virtual_min', None), keys)
        with self.assertRaises(ValueError):
            aggregate_run_offsets([entry('first.fsa', source)], kit())

    def test_rejected_marker_keys_are_not_used(self):
        aggregated = aggregate_run_offsets([
            entry('rejected.fsa', result({'A': 99}, 99, 99, marker_status='rejected')),
            entry('accepted.fsa', result({'A': 3}, 3, 3)),
        ], kit())
        self.assertEqual(aggregated['markers']['ExampleM']['bin_offsets']['A'], 3)

    def test_aggregate_is_usable_by_sample_caller_without_mutation(self):
        definition = kit()
        entries = [entry('first.fsa', result(dict.fromkeys('ABCD', .3), .3, .3)),
                   entry('second.fsa', result(dict.fromkeys('ABCD', .5), .5, .5))]
        before = deepcopy((entries, definition))
        aggregated = aggregate_run_offsets(entries, definition)
        peaks = [{'channel': 'DATA1', 'scan': 4000, 'size_bp': 175.6, 'height': 1000}]
        called = call_sample(peaks, definition, aggregated, sizing_accepted=True)
        self.assertEqual(called['status'], 'tentative')
        self.assertEqual([c['label'] for c in called['markers'][0]['calls']], ['B'])
        self.assertEqual((entries, definition), before)
        json.dumps(aggregated, allow_nan=False)

    def test_bounded_ascii_identity_and_finite_offsets_are_required(self):
        for identifier, quality, source in (
            ('не-ascii.fsa', 0, result({'A': 1}, 1, 1)),
            ('valid.fsa', True, result({'A': 1}, 1, 1)),
            ('valid.fsa', float('nan'), result({'A': 1}, 1, 1)),
            ('valid.fsa', 0, result({'A': float('inf')}, 1, 1)),
            ('valid.fsa', 0, result({'A': True}, 1, 1)),
        ):
            with self.subTest(identifier=identifier, quality=quality, source=source):
                with self.assertRaises(ValueError):
                    aggregate_run_offsets([entry(identifier, source, quality)], kit())


if __name__ == '__main__':
    unittest.main()
