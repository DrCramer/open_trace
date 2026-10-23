"""Independent schema, input bounds and ambiguity-preserving kit contracts."""
import copy
import json
import unittest

from genemapper_open.kit_definition import load_kit_definition, MAX_JSON_BYTES


def kit_fixture():
    return {'schema_version': 1, 'name': 'Synthetic kit', 'markers': [{
        'name': 'M', 'channel': 'DATA1', 'min_bp': 98, 'max_bp': 110, 'repeat': 4,
        'bins': [{'label': str(index), 'center_bp': center, 'left_bp': .5, 'right_bp': .5}
                 for index, center in enumerate((100, 104, 108), 1)],
        'ladder_labels': ['1', '2', '3'], 'parameters': {
            'ladder_cutoff_ratio': .1, 'cutoff_ratio': .2,
            'minus_a_ratio': .5, 'minus_a_min_bp': 0, 'minus_a_max_bp': 1.5,
            'backward_stutter_ranges': [{'min_bp': 3.5, 'max_bp': 4.5, 'ratio': .1}],
            'forward_stutter_ranges': []}}]}


class KitDefinitionTests(unittest.TestCase):
    def test_json_bytes_mapping_roundtrip_and_no_mutation(self):
        original = kit_fixture()
        before = copy.deepcopy(original)
        expected = load_kit_definition(original)
        for source in (json.dumps(original), json.dumps(original).encode(), expected):
            self.assertEqual(load_kit_definition(source), expected)
        self.assertEqual(original, before)
        self.assertEqual(expected['markers'][0]['parameters']['clear_ranges'], [])
        self.assertEqual(expected['markers'][0]['bins'][0]['confidence_weight'], 1)

    def test_ladder_labels_resolve_case_insensitively(self):
        kit = kit_fixture()
        kit['markers'][0]['bins'][0]['label'] = 'A1'
        kit['markers'][0]['ladder_labels'][0] = 'a1'
        self.assertEqual(load_kit_definition(kit)['markers'][0]['ladder_labels'][0], 'A1')

    def test_touching_and_overlapping_bins_are_allowed_with_diagnostics(self):
        kit = kit_fixture()
        kit['markers'][0]['bins'][0]['right_bp'] = 3.5
        result = load_kit_definition(kit)
        self.assertEqual(result['diagnostics'][0]['pair_count'], 1)
        self.assertEqual(result['diagnostics'][0]['examples'], [['1', '2']])

    def test_no_ladder_definitions_is_explicitly_representable(self):
        kit = kit_fixture()
        kit['markers'][0]['ladder_labels'] = []
        self.assertEqual(load_kit_definition(kit)['markers'][0]['ladder_labels'], [])

    def test_required_lab_settings_are_never_invented(self):
        for field in kit_fixture()['markers'][0]['parameters']:
            kit = kit_fixture()
            del kit['markers'][0]['parameters'][field]
            with self.subTest(field=field), self.assertRaises(ValueError):
                load_kit_definition(kit)

    def test_nonfinite_boolean_negative_and_unordered_numeric_fields(self):
        cases = [('min_bp', True), ('min_bp', float('nan')), ('max_bp', float('inf')),
                 ('min_bp', -1), ('min_bp', 110), ('repeat', 4.0), ('repeat', 0)]
        for key, value in cases:
            kit = kit_fixture()
            kit['markers'][0][key] = value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                load_kit_definition(kit)
        for key, value in [('left_bp', -1), ('confidence_weight', 2), ('center_bp', 112)]:
            kit = kit_fixture()
            kit['markers'][0]['bins'][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                load_kit_definition(kit)

    def test_explicit_ratio_and_range_boundaries(self):
        kit = kit_fixture()
        params = kit['markers'][0]['parameters']
        params.update(ladder_cutoff_ratio=0, cutoff_ratio=1, minus_a_ratio=0,
                      minus_a_min_bp=0, minus_a_max_bp=0)
        params['forward_stutter_ranges'] = [{'min_bp': 0, 'max_bp': 0, 'ratio': 0}]
        load_kit_definition(kit)
        for key, value in [('minus_a_ratio', 1.01), ('minus_a_min_bp', 2),
                           ('backward_stutter_ranges', [{'min_bp': 5, 'max_bp': 4, 'ratio': .1}])]:
            bad = copy.deepcopy(kit)
            bad['markers'][0]['parameters'][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                load_kit_definition(bad)

    def test_unique_names_channels_labels_and_expected_order(self):
        base = kit_fixture()
        mutations = [lambda k: k['markers'].append(copy.deepcopy(k['markers'][0])),
                     lambda k: k['markers'][0].update(channel='DATA01'),
                     lambda k: k['markers'][0].update(channel='DATA0'),
                     lambda k: k['markers'][0].update(name=' M'),
                     lambda k: k['markers'][0]['bins'][1].update(label='1'),
                     lambda k: k['markers'][0].update(ladder_labels=['3', '2', '1']),
                     lambda k: k['markers'][0].update(ladder_labels=['1', '1']),
                     lambda k: k['markers'][0].update(ladder_labels=['missing'])]
        for index, mutation in enumerate(mutations):
            bad = copy.deepcopy(base)
            mutation(bad)
            with self.subTest(index=index), self.assertRaises(ValueError):
                load_kit_definition(bad)

    def test_unknown_duplicate_and_nonfinite_json_fields_are_rejected(self):
        for source in ('{"schema_version":1,"schema_version":1}', '{"x":NaN}',
                       '{"x":Infinity}', 'not a file path.json', b'\xff'):
            with self.subTest(source=source), self.assertRaises(ValueError):
                load_kit_definition(source)
        bad = kit_fixture()
        bad['vendor_code'] = 'never imported'
        with self.assertRaises(ValueError):
            load_kit_definition(bad)

    def test_bounded_input_and_deep_or_cyclic_json(self):
        for source in (' '*(MAX_JSON_BYTES+1), '['*2000+'0'+']'*2000):
            with self.assertRaises(ValueError):
                load_kit_definition(source)
        cyclic = {}; cyclic['self'] = cyclic
        with self.assertRaises(ValueError):
            load_kit_definition(cyclic)
        for field, count in [('markers', 129), ('bins', 129), ('ladder_labels', 33)]:
            bad = kit_fixture()
            owner = bad if field == 'markers' else bad['markers'][0]
            owner[field] *= count
            with self.subTest(field=field), self.assertRaises(ValueError):
                load_kit_definition(bad)


if __name__ == '__main__':
    unittest.main()
