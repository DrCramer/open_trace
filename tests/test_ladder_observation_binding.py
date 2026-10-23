from copy import deepcopy
import json
import unittest
from unittest.mock import patch

from genemapper_open.ladder import match_ladder
from genemapper_open.ladder_calling import ladder_call_observations
from genemapper_open.ladder_observation_binding import bind, validate
from genemapper_open.spike_adjustment import adjust_ladder_spikes, apply_peak_states
from genemapper_open.str_demo import make_str_demo


def fixture():
    kit = make_str_demo()['kit']
    peaks = [{'channel': 'DATA1', 'scan': 200+20*bp, 'size_bp': bp,
              'height': 1000, 'offscaled': False, 'id': 'source-'+str(index)}
             for index, bp in enumerate((168.3, 176.3, 184.3, 192.3))]
    peaks.append({'channel': 'DATA1', 'scan': 3280, 'size_bp': 154.,
                  'height': 100., 'offscaled': False, 'id': 'spike-source', 'spike': True})
    matching = match_ladder(peaks, kit, size_standard_max_bp=260, sizing_accepted=True)
    observations = ladder_call_observations(peaks, kit, matching)
    return peaks, kit, matching, bind(peaks, kit, matching, observations)


POLICY = {'sample_cutoff': True, 'sample_marker_envelope': True, 'ladder_height_ratio': .2}


class LadderObservationBindingTests(unittest.TestCase):
    def test_valid_binding_returns_own_copy_without_mutating_inputs(self):
        peaks, kit, matching, observations = fixture()
        before = deepcopy((peaks, kit, matching, observations))
        result = validate(peaks, kit, matching, observations)
        self.assertEqual(result, observations)
        self.assertIsNot(result, observations)
        result['markers'][0]['calls'][0]['label'] = 'changed'
        self.assertEqual((peaks, kit, matching, observations), before)
        json.dumps(observations, allow_nan=False)
        self.assertEqual(observations['observation_binding']['scope'],
                         'consistency_only_not_caller_authentication')

    def test_added_offscale_source_rejects_stale_window_links(self):
        peaks, kit, matching, observations = fixture()
        peaks.append({'channel': 'DATA1', 'scan': 3300, 'size_bp': 155.,
                      'height': 10., 'offscaled': True})
        with self.assertRaisesRegex(ValueError, 'source inputs'):
            validate(peaks, kit, matching, observations)

    def test_changed_kit_matching_and_source_identity_are_rejected(self):
        mutations = [
            lambda p, k, m, o: k['markers'][0].update(min_bp=159),
            lambda p, k, m, o: m['markers']['ExampleM'].update(confidence=.2),
            lambda p, k, m, o: p[0].update(id='different-source'),
            lambda p, k, m, o: p[0].update(scan=p[0]['scan']+1),
            lambda p, k, m, o: p[0].update(size_bp=p[0]['size_bp']+.01),
            lambda p, k, m, o: p[0].update(height=999),
            lambda p, k, m, o: p[0].update(channel='DATA2'),
            lambda p, k, m, o: p[0].update(offscaled=True),
            lambda p, k, m, o: p[0].update(pullup=True),
            lambda p, k, m, o: p.reverse(),
        ]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                args = fixture()
                mutation(*args)
                with self.assertRaises(ValueError):
                    validate(*args)

    def test_calls_confidence_window_and_full_peak_links_are_immutable(self):
        mutations = [
            lambda o: o['markers'][0]['calls'][0].update(label='wrong'),
            lambda o: o['markers'][0]['calls'][0].update(confidence=.2),
            lambda o: o['markers'][0]['calls'].pop(),
            lambda o: o['markers'][0]['window_bp'].__setitem__(0, 160),
            lambda o: o['markers'][0]['peak_review'].pop(),
            lambda o: o['markers'][0]['peak_review'][0].update(peak_index=3),
        ]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                peaks, kit, matching, observations = fixture()
                mutation(observations)
                with self.assertRaisesRegex(ValueError, 'observation structure'):
                    validate(peaks, kit, matching, observations)

    def test_valid_adjustment_accepts_initial_and_applied_source_flags_without_recalling(self):
        peaks, kit, matching, observations = fixture()
        adjusted = adjust_ladder_spikes(peaks, observations, POLICY)
        applied = apply_peak_states(peaks, adjusted['spike_adjustments'])
        before = deepcopy((peaks, matching, observations, adjusted, applied))
        self.assertFalse(applied[4]['spike'])
        self.assertEqual(adjusted['markers'][0]['calls'], observations['markers'][0]['calls'])
        with patch('genemapper_open.ladder.match_ladder', side_effect=AssertionError('rematching')), \
                patch('genemapper_open.ladder_calling.ladder_call_observations', side_effect=AssertionError('recalling')), \
                patch('genemapper_open.allele_calling.call_sample', side_effect=AssertionError('sample calling')):
            self.assertEqual(validate(peaks, kit, matching, adjusted), adjusted)
            self.assertEqual(validate(applied, kit, matching, adjusted), adjusted)
        self.assertEqual((peaks, matching, observations, adjusted, applied), before)

    def test_normalized_flag_representations_survive_spike_adjustment(self):
        for flags in (['spike', 'custom'], {'spike': True, 'custom': True}):
            with self.subTest(flags=flags):
                peaks, kit, matching, observations = fixture()
                peaks[4].pop('spike')
                peaks[4]['flags'] = flags
                observations = bind(peaks, kit, matching,
                                    ladder_call_observations(peaks, kit, matching))
                adjusted = adjust_ladder_spikes(peaks, observations, POLICY)
                applied = apply_peak_states(peaks, adjusted['spike_adjustments'])
                self.assertEqual(validate(applied, kit, matching, adjusted), adjusted)

    def test_changed_adjustment_summary_metric_and_source_metadata_are_rejected(self):
        for mutation in (
            lambda p, o: o['spike_adjustments'].update(calling_repeated=True),
            lambda p, o: o['spike_adjustments']['peak_states'][4].update(initial_spike=False),
            lambda p, o: o['spike_adjustments']['peak_states'][4].update(current_spike=True),
            lambda p, o: o['spike_adjustments']['peak_states'].pop(),
            lambda p, o: o['markers'][0]['spike_state'].update(metric=1),
            lambda p, o: p[4]['spike_adjustment'].update(events=[]),
            lambda p, o: p[4].update(offscaled=True),
            lambda p, o: p[0].update(spike=True),
        ):
            with self.subTest(mutation=mutation):
                peaks, kit, matching, observations = fixture()
                adjusted = adjust_ladder_spikes(peaks, observations, POLICY)
                applied = apply_peak_states(peaks, adjusted['spike_adjustments'])
                mutation(applied, adjusted)
                with self.assertRaises(ValueError):
                    validate(applied, kit, matching, adjusted)

    def test_phase_metadata_without_summary_and_unbound_observations_are_rejected(self):
        peaks, kit, matching, observations = fixture()
        unbound = deepcopy(observations)
        unbound.pop('observation_binding')
        with self.assertRaises(ValueError):
            validate(peaks, kit, matching, unbound)
        observations['markers'][0]['spike_state'] = {'metric': 0}
        with self.assertRaises(ValueError):
            validate(peaks, kit, matching, observations)

    def test_binding_cannot_be_created_after_adjustment(self):
        peaks, kit, matching, observations = fixture()
        adjusted = adjust_ladder_spikes(peaks, observations, POLICY)
        with self.assertRaisesRegex(ValueError, 'before spike adjustment'):
            bind(peaks, kit, matching, adjusted)


if __name__ == '__main__':
    unittest.main()
