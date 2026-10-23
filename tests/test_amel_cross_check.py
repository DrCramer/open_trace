from copy import deepcopy
import unittest
from genemapper_open.amel_cross_check import evaluate_amel_cross_check


def marker(name, labels, *, y=False, gq=1, off_ladder=False):
    return {'name': name, 'is_y_marker': y, 'gq': gq,
            'calls': [{'label': label, 'off_ladder': off_ladder} for label in labels]}


class AmelCrossCheckTests(unittest.TestCase):
    def score(self, markers, **kwargs):
        return evaluate_amel_cross_check(markers, sample_type=kwargs.get('sample_type', 'Sample'),
                                         weight=kwargs.get('weight', .5))

    def test_x_and_xy_y_presence_matrix(self):
        for labels, ylabels, expected in ((['X'], [], False), (['X'], ['1'], True),
                (['X', 'Y'], [], True), (['X', 'Y'], ['1'], False)):
            result = self.score([marker('AMEL', labels), marker('Y1', ylabels, y=True)])
            self.assertEqual(result['status'], 'evaluated_subset')
            self.assertIs(result['mismatch'], expected)
            self.assertEqual([u['gq_after'] for u in result['updates']], [.5 if expected else 1]*2)

    def test_invalid_combinations_and_case_insensitive_calls(self):
        for labels in ([], ['Y'], ['Z'], ['X', 'X'], ['X', 'Y', 'Z'], ['X', 'Y', 'Y']):
            result = self.score([marker('AMEL', labels), marker('Y1', ['1'], y=True)])
            self.assertEqual(result['reason'], 'invalid_amel_combination', labels)
            self.assertTrue(result['mismatch'])
        self.assertFalse(self.score([marker('Amelogenin', ['x', 'y']), marker('Y1', ['1'], y=True)])['mismatch'])

    def test_off_ladder_y_calls_do_not_count_but_amel_uses_all_calls(self):
        for labels, expected in ((['X'], False), (['X', 'Y'], True)):
            result = self.score([marker('AMEL', labels), marker('Y1', ['OL'], y=True, off_ladder=True)])
            self.assertIs(result['mismatch'], expected)
        result = self.score([marker('AMEL', ['X', 'OL']), marker('Y1', [], y=True)])
        self.assertTrue(result['mismatch'])

    def test_first_mismatch_flags_all_y_and_amel_only(self):
        markers = [marker('AMEL', ['X'], gq=.8), marker('Auto', ['A']),
                   marker('Y1', ['1'], y=True, gq=.6), marker('Y2', [], y=True, gq=.4)]
        before = deepcopy(markers)
        result = self.score(markers)
        self.assertEqual(result['first_mismatch_marker'], 'Y1')
        self.assertEqual([u['marker'] for u in result['updates']], ['Y1', 'Y2', 'AMEL'])
        self.assertEqual([u['gq_after'] for u in result['updates']], [.3, .2, .4])
        self.assertEqual(markers, before)
        self.assertFalse(result['call_confidences_recomputed'])

    def test_zero_weight_still_flags_and_full_weight_zeros_gq(self):
        observations = [marker('AMEL', ['X']), marker('Y1', ['1'], y=True)]
        zero = self.score(observations, weight=0)
        self.assertEqual([(u['metric'], u['gq_after']) for u in zero['updates']], [(1, 1)]*2)
        self.assertEqual([u['gq_after'] for u in self.score(observations, weight=1)['updates']], [0, 0])

    def test_validator_entry_gate_is_case_sensitive_processor_is_not(self):
        lower = self.score([marker('amel', ['X']), marker('Y1', ['1'], y=True)])
        self.assertEqual(lower['reason'], 'validator_exact_amel_name_missing')
        mixed = self.score([marker('amel', ['X']), marker('AMEL', ['X', 'Y']), marker('Y1', ['1'], y=True)])
        self.assertEqual(mixed['amel_marker'], 'amel')
        self.assertTrue(mixed['mismatch'])
        blocked=self.score([marker('amel',['X']),marker('AMEL',['X'],gq=None),marker('Y1',['1'],y=True)])
        self.assertEqual(blocked['reason'],'validator_amel_quality_missing')

    def test_excluded_types_no_y_and_missing_metrics_do_not_pass(self):
        observations = [marker('AMEL', ['X']), marker('Y1', ['1'], y=True)]
        for kind in ('Negative Control', 'Allelic Ladder'):
            self.assertEqual(self.score(observations, sample_type=kind)['reason'], 'sample_type_excluded')
        self.assertEqual(self.score([marker('AMEL', ['X'])])['reason'], 'no_y_markers')
        observations[1]['gq'] = None
        self.assertEqual(self.score(observations)['status'], 'unavailable')
        self.assertEqual(self.score(observations)['updates'], [])

    def test_invalid_inputs(self):
        for weight in (True, -.1, 1.1, float('nan')):
            with self.assertRaises(ValueError): self.score([], weight=weight)
        with self.assertRaises(ValueError): self.score([marker('AMEL', ['X'])]*2)
        with self.assertRaises(ValueError): self.score([marker('AMEL', ['Х'])])
