from contextlib import ExitStack
from copy import deepcopy
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import urllib.error
import urllib.request

from genemapper_open.server import analyze
from genemapper_open.str_demo import (make_pullup_demo, make_pullup_demo_definition,
    make_true_peak_demo, make_quality_demo_definition, make_ladder_quality_demo_definition)
from genemapper_open.str_workflow import run_str_workflow, compact_analysis
from genemapper_open.run_workflow import run_str_run_workflow
from genemapper_open.marker_quality_workflow import attach_marker_quality
from genemapper_open.pullup_quality import attach_pullup_quality
from genemapper_open.project_store import ProjectStore
from genemapper_open.project_runs import create_project_run, revise_project_run


ROOT = Path(__file__).resolve().parents[1]


class PullupWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.cases = {}
        for name, options in (('detected', {}), ('flags_only', {'detector': False}),
                              ('unrequested', {'requested': False})):
            fixture = make_pullup_demo(**options)
            sample = analyze(fixture['sample_bytes'], 'sample.fsa', **fixture['settings'])
            ladder = analyze(fixture['ladder_bytes'], 'ladder.fsa', **fixture['settings'])
            report = run_str_workflow(sample, ladder, fixture['kit'],
                ladder_quality_definition=make_ladder_quality_demo_definition())
            cls.cases[name] = {'fixture': fixture, 'sample': sample, 'ladder': ladder, 'report': report}

    def test_same_bytes_keep_bc_without_detector_and_only_c_with_explicit_route(self):
        active = self.cases['detected']
        for name in ('flags_only', 'unrequested'):
            case = self.cases[name]
            self.assertEqual(case['fixture']['sample_bytes'], active['fixture']['sample_bytes'])
            self.assertEqual(case['fixture']['ladder_bytes'], active['fixture']['ladder_bytes'])
            self.assertEqual(case['sample']['source_sha256'], active['sample']['source_sha256'])
            self.assertEqual([c['label'] for c in case['report']['calls']], ['B', 'C'])
            self.assertTrue(all(not p.get('pullup', False) for p in case['sample']['channels'][0]['peaks']))
        self.assertEqual([c['label'] for c in active['report']['calls']], ['C'])
        flags_only = self.cases['flags_only']['report']['quality']['pullup']
        self.assertEqual(flags_only['markers'][0]['metric'], 0)
        self.assertEqual(flags_only['markers'][0]['triggers'], [])
        self.assertEqual(flags_only['profile']['code'], 0)
        self.assertEqual(self.cases['flags_only']['sample']['pullup_detection']['status'], 'not_requested')
        self.assertNotIn('pullup', self.cases['unrequested']['report']['quality'])
        self.assertNotIn('pullup_definition', self.cases['unrequested']['sample']['peak_settings'])

    def test_detection_keeps_sizing_reference_waveforms_and_peak_geometry(self):
        before = self.cases['unrequested']['sample']
        after = self.cases['detected']['sample']
        self.assertEqual(before['sizing'], after['sizing'])
        reference = after['sizing']['reference_channel']
        old_channels = {c['tag']: c for c in before['channels']}
        for channel in after['channels']:
            old = old_channels[channel['tag']]
            self.assertEqual(channel.get('values'), old.get('values'))
            self.assertEqual(len(channel['peaks']), len(old['peaks']))
            if channel['tag'] == reference:
                self.assertEqual(channel, old)
            for a, b in zip(channel['peaks'], old['peaks']):
                for key in ('scan', 'height', 'size_bp', 'begin', 'end', 'fwhm_scan',
                            'raw_height', 'area', 'area_bp', 'offscaled', 'sg_scan'):
                    self.assertEqual(a.get(key), b.get(key), (channel['tag'], key))
        self.assertTrue(after['artifacts']['reference_channel_unchanged'])
        self.assertTrue(after['artifacts']['automatic_pullup_applied'])
        self.assertFalse(after['pullup_detection']['original_automatic_route_confirmed'])

    def test_detected_b_is_retained_as_linked_peak_with_measured_trigger(self):
        case = self.cases['detected']
        report = case['report']
        first = case['sample']['channels'][0]['peaks'][0]
        history = first['pullup_classification']
        self.assertTrue(first['pullup'])
        self.assertEqual(history['matched_reference_id'], 'DATA2:0')
        self.assertEqual(history['state']['artifact_label'], 'SPU')
        quality = report['quality']['pullup']
        self.assertEqual((quality['markers'][0]['metric'], quality['profile']['code']), (1, 2))
        self.assertEqual(quality['markers'][0]['linked_peak_count'], 2)
        self.assertEqual(len(report['peaks']), 2)
        self.assertIn('pullup', report['allele_calling']['markers'][0]['peak_review'][0]['flags'])
        trigger = quality['markers'][0]['triggers'][0]
        self.assertEqual((trigger['id'], trigger['ordinal'], trigger['source_peak_id']),
                         ('PTI_BLEED_THROUGH', 4, 'DATA1:0'))
        self.assertEqual(trigger['ratio'], history['state']['ratio'])
        self.assertEqual(trigger['thresholds'], [.08])
        self.assertEqual(trigger['positions_bp'], [first['size_bp']])
        self.assertEqual(trigger['ratio_provenance'], 'measured_pair')
        self.assertTrue(trigger['ratio_available'])
        self.assertTrue(report['provenance']['experimental_pullup_routing_applied'])
        self.assertFalse(report['provenance']['original_automatic_pullup_route_confirmed'])
        json.dumps(report, allow_nan=False)

    def test_existing_flag_is_consumed_without_new_detection_or_ratio_inference(self):
        case = self.cases['flags_only']
        sample = deepcopy(case['sample'])
        sample['channels'][0]['peaks'][0]['pullup'] = True
        with patch('genemapper_open.pullup.apply_pullup_detection',
                   side_effect=AssertionError('Retained flags must not trigger detection')):
            report = run_str_workflow(sample, case['ladder'], case['fixture']['kit'],
                ladder_quality_definition=make_ladder_quality_demo_definition())
        self.assertEqual([c['label'] for c in report['calls']], ['C'])
        quality = report['quality']['pullup']
        self.assertEqual(quality['profile']['code'], 2)
        trigger = quality['markers'][0]['triggers'][0]
        self.assertEqual((trigger['ratio'], trigger['thresholds']), (-1, [-1]))
        self.assertFalse(trigger['ratio_available'])
        self.assertFalse(report['provenance']['automatic_pullup_applied'])

    def test_profile_fail_does_not_penalize_gq_or_ladder_admission(self):
        case = self.cases['detected']
        report = attach_marker_quality(case['report'], make_quality_demo_definition())
        self.assertEqual(report['quality']['pullup']['profile']['code'], 2)
        self.assertEqual(report['quality']['pullup']['markers'][0]['metric'], 1)
        self.assertEqual(report['quality']['marker_gq']['markers'][0]['score']['gq'], 1)
        self.assertEqual(report['quality']['marker_gq']['sample_category'], 0)
        self.assertEqual(report['quality']['ladder_gq']['al_subset_code'], 0)
        self.assertTrue(report['quality']['ladder_gq']['gate_passed'])
        definition = deepcopy(case['fixture']['settings']['pullup_definition'])
        definition['profile_threshold'] = 1
        refreshed = attach_pullup_quality(report, definition)
        self.assertEqual(refreshed['quality']['pullup']['profile']['code'], 1)
        self.assertEqual(refreshed['quality']['marker_gq'], report['quality']['marker_gq'])
        self.assertEqual(refreshed['quality']['ladder_gq'], report['quality']['ladder_gq'])
        self.assertEqual(refreshed['calls'], report['calls'])
        for name in ('gq_or_confidence_changed', 'ladder_admission_changed', 'summary_pass_changed'):
            self.assertFalse(refreshed['quality']['pullup'][name])

    def test_explicit_phase_uses_height_after_true_peak_restoration(self):
        fixture = make_true_peak_demo(pullup_reference_height=18000)
        fixture['settings']['pullup_definition'] = make_pullup_demo_definition()
        sample = analyze(fixture['sample_bytes'], 'restored-pullup.fsa', **fixture['settings'])
        first = sample['channels'][0]['peaks'][0]
        reference = next(c for c in sample['channels'] if c['tag'] == 'DATA2')['peaks'][0]
        self.assertEqual(first['true_peak_restoration']['status'], 'restored')
        event = first['pullup_classification']['events'][0]
        self.assertAlmostEqual(event['attempt_ratio'], first['height']/reference['height'])
        self.assertNotAlmostEqual(event['attempt_ratio'],
            first['true_peak_restoration']['before']['height']/reference['height'])
        self.assertEqual(first['true_peak_restoration']['after']['height'], first['height'])

    def test_run_and_compact_observations_recalculate_calls_without_detection(self):
        case = self.cases['detected']
        before = deepcopy((case['sample'], case['ladder']))
        with patch('genemapper_open.pullup.apply_pullup_detection',
                   side_effect=AssertionError('No detector during run calling')):
            report = run_str_run_workflow(case['sample'],
                [{'ladder_id': 'Own.fsa', 'analysis': case['ladder']}], case['fixture']['kit'],
                ladder_quality_definition=make_ladder_quality_demo_definition())
            compact = run_str_run_workflow(compact_analysis(case['sample']),
                [{'ladder_id': 'Own.fsa', 'analysis': compact_analysis(case['ladder'])}], case['fixture']['kit'],
                ladder_quality_definition=make_ladder_quality_demo_definition())
        self.assertEqual(report, compact)
        self.assertEqual((case['sample'], case['ladder']), before)
        self.assertEqual([c['label'] for c in report['calls']], ['C'])
        self.assertEqual(report['quality']['pullup']['profile']['code'], 2)
        self.assertEqual(report['quality']['run_ladders'][0]['quality_code'], 0)
        self.assertTrue(report['provenance']['experimental_pullup_routing_applied'])

    def test_persistent_revision_and_import_reuse_detection_history(self):
        case = self.cases['detected']
        inputs = {'schema_version': 1, 'kit': case['fixture']['kit'],
            'ladder_quality_definition': make_ladder_quality_demo_definition(), 'missing_offset_policy': 'reject',
            'ladders': [{'ladder_id': 'Own.fsa', 'analysis': case['ladder']}],
            'samples': [{'sample_key': 'Own', 'analysis': case['sample'],
                         'marker_quality_definition': make_quality_demo_definition()}]}
        with ExitStack() as stack:
            stack.enter_context(patch('genemapper_open.pullup.apply_pullup_detection',
                side_effect=AssertionError('No repeated detector for retained analysis')))
            stack.enter_context(patch('genemapper_open.server.analyze',
                side_effect=AssertionError('No FSA analysis for retained run')))
            with ProjectStore.memory('Own pull-up history') as store:
                created = create_project_run(store, 'Run', inputs, actor='Tester', reason='Own fixture')
                old = store.get_analysis(created['outputs'][0]['analysis_id'])
                changed = revise_project_run(store, created['run_id'], created['revision_id'], inputs,
                    actor='Tester', reason='Recalculate calls only')
                new = store.get_analysis(changed['outputs'][0]['analysis_id'])
                self.assertEqual(old['result']['peaks'], new['result']['peaks'])
                self.assertEqual(old['result']['quality']['pullup'], new['result']['quality']['pullup'])
                self.assertEqual([c['label'] for c in new['result']['calls']], ['C'])
                self.assertEqual(store.get_analysis(created['outputs'][0]['analysis_id']), old)
                exported = store.export_json()
            self.assertNotIn('"values":', exported)
            with ProjectStore.from_json(exported) as imported:
                self.assertEqual(imported.get_analysis(changed['outputs'][0]['analysis_id']), new)
                retained = imported.get_run_revision(changed['revision_id'])['inputs']
                again = revise_project_run(imported, created['run_id'], changed['revision_id'], retained,
                    actor='Tester', reason='Recalculate imported observations')
                final = imported.get_analysis(again['outputs'][0]['analysis_id'])['result']
                self.assertEqual(final['peaks'], new['result']['peaks'])
                self.assertEqual(final['quality']['pullup']['profile']['code'], 2)

    def test_missing_requested_processing_metadata_rejects_calling(self):
        case = self.cases['detected']
        sample = deepcopy(case['sample'])
        sample['channels'][0]['peaks'][0].pop('pullup_classification')
        report = run_str_workflow(sample, case['ladder'], case['fixture']['kit'])
        self.assertEqual(report['status'], 'rejected')
        self.assertEqual(report['calls'], [])
        self.assertTrue(any('pull-up processing unavailable' in reason for reason in report['reasons']))

    def test_nonboolean_flag_and_nonfinite_retained_ratio_are_rejected(self):
        case = self.cases['detected']
        for field in ('flag', 'ratio'):
            with self.subTest(field=field):
                sample = deepcopy(case['sample'])
                peak = sample['channels'][0]['peaks'][0]
                if field == 'flag': peak['pullup'] = 'true'
                else: peak['pullup_classification']['state']['ratio'] = float('nan')
                with self.assertRaises(ValueError):
                    run_str_workflow(sample, case['ladder'], case['fixture']['kit'])

    def test_invalid_explicit_route_metadata_is_rejected_before_analysis(self):
        fixture = self.cases['detected']['fixture']
        for change in ('phase', 'same_channel', 'reference_candidate', 'boolean_distance'):
            with self.subTest(change=change):
                settings = deepcopy(fixture['settings'])
                detector = settings['pullup_definition']['detector']
                if change == 'phase': detector['phase'] = 'before_sizing'
                elif change == 'same_channel': detector['channel_pairs'][0]['reference_channel'] = 'DATA1'
                elif change == 'reference_candidate': detector['channel_pairs'][0]['candidate_channel'] = 'DATA106'
                else: detector['max_scan_distance'] = True
                with self.assertRaises(ValueError):
                    analyze(fixture['sample_bytes'], 'invalid.fsa', **settings)

    def test_sample_ladder_policy_mismatch_is_not_silently_accepted(self):
        case = self.cases['detected']
        report = run_str_workflow(case['sample'], self.cases['flags_only']['ladder'], case['fixture']['kit'])
        self.assertEqual(report['status'], 'rejected')
        self.assertEqual(report['calls'], [])
        self.assertTrue(any('identical explicit analysis/standard settings' in reason for reason in report['reasons']))

    def test_cli_variants_export_same_bytes_and_explicit_policy(self):
        with tempfile.TemporaryDirectory() as directory:
            for variant, name, labels, code in (
                    ('pullup', 'detected', ['C'], 2),
                    ('pullup-flags-only', 'flags_only', ['B', 'C'], 0),
                    ('pullup-unrequested', 'unrequested', ['B', 'C'], None)):
                with self.subTest(variant=variant):
                    target = Path(directory)/variant
                    process = subprocess.run([sys.executable, '-m', 'genemapper_open', 'str-demo',
                        str(target), '--variant', variant], cwd=ROOT, capture_output=True, text=True, timeout=20)
                    self.assertEqual(process.returncode, 0, process.stderr)
                    report = json.loads((target/'report.json').read_text(encoding='utf-8'))
                    self.assertEqual([c['label'] for c in report['calls']], labels)
                    self.assertEqual((target/'sample.fsa').read_bytes(), self.cases[name]['fixture']['sample_bytes'])
                    self.assertEqual((target/'ladder.fsa').read_bytes(), self.cases[name]['fixture']['ladder_bytes'])
                    self.assertEqual(json.loads((target/'settings.json').read_text()), self.cases[name]['fixture']['settings'])
                    if code is None:
                        self.assertNotIn('pullup', report['quality'])
                        self.assertFalse((target/'pullup.json').exists())
                    else:
                        self.assertEqual(report['quality']['pullup']['profile']['code'], code)
                        self.assertEqual(json.loads((target/'pullup.json').read_text()),
                            self.cases[name]['fixture']['settings']['pullup_definition'])

    def test_standalone_cli_quality_reuses_flags_without_detector_or_call_changes(self):
        report = attach_marker_quality(self.cases['detected']['report'], make_quality_demo_definition())
        definition = make_pullup_demo_definition(detector=False)
        definition['profile_threshold'] = 1
        with tempfile.TemporaryDirectory() as directory:
            source, recipe, output = (Path(directory)/name for name in ('input.json', 'recipe.json', 'output.json'))
            source.write_text(json.dumps(report, allow_nan=False), encoding='utf-8')
            recipe.write_text(json.dumps(definition), encoding='utf-8')
            process = subprocess.run([sys.executable, '-m', 'genemapper_open', 'pullup-quality', str(source),
                '--definition', str(recipe), '--output', str(output)], cwd=ROOT,
                capture_output=True, text=True, timeout=15)
            self.assertEqual(process.returncode, 0, process.stderr)
            refreshed = json.loads(output.read_text(encoding='utf-8'))
        self.assertEqual(refreshed['quality']['pullup']['profile']['code'], 1)
        self.assertEqual(refreshed['calls'], report['calls'])
        self.assertEqual(refreshed['peaks'], report['peaks'])
        self.assertEqual(refreshed['quality']['marker_gq'], report['quality']['marker_gq'])
        self.assertEqual(refreshed['quality']['ladder_gq'], report['quality']['ladder_gq'])


class PullupHttpWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server_log = tempfile.TemporaryFile(mode='w+', encoding='utf-8')
        cls.process = subprocess.Popen([sys.executable, '-u', '-m', 'genemapper_open', 'serve', '--port', '0'],
            cwd=ROOT, stdout=subprocess.PIPE, stderr=cls.server_log, text=True, encoding='utf-8')
        line = cls.process.stdout.readline()
        match = re.search(r'http://127\.0\.0\.1:\d+/', line)
        if not match:
            cls.process.terminate(); cls.process.wait(timeout=5)
            cls.process.stdout.close(); cls.server_log.close()
            raise RuntimeError('Pull-up test server did not start: '+line)
        cls.base = match.group(0)
        with urllib.request.urlopen(cls.base, timeout=5) as response:
            cls.token = re.search(r"const TOKEN='([^']+)'", response.read().decode()).group(1)

    @classmethod
    def tearDownClass(cls):
        cls.process.terminate(); cls.process.wait(timeout=5)
        cls.process.stdout.close(); cls.server_log.close()

    def post(self, path, payload, expected=200):
        request = urllib.request.Request(self.base+path, data=json.dumps(payload, allow_nan=False).encode(),
            headers={'X-Session-Token': self.token, 'Content-Type': 'application/json'}, method='POST')
        try: response = urllib.request.urlopen(request, timeout=20)
        except urllib.error.HTTPError as error: response = error
        with response:
            body = json.load(response)
            self.assertEqual(response.code, expected, body)
            return body

    def test_demo_routes_expose_detected_and_flags_only_results(self):
        for variant, labels, code in (('pullup', ['C'], 2), ('pullup-flags-only', ['B', 'C'], 0),
                                      ('pullup-unrequested', ['B', 'C'], None)):
            with self.subTest(variant=variant):
                demo = self.post('api/demo-str?variant='+variant, {})
                report = demo['report']
                self.assertEqual([c['label'] for c in report['calls']], labels)
                if code is None: self.assertNotIn('pullup', report['quality'])
                else: self.assertEqual(report['quality']['pullup']['profile']['code'], code)

    def test_standalone_quality_http_route_retains_calls_and_rejects_invalid_recipe(self):
        demo = self.post('api/demo-str?variant=pullup', {})
        definition = make_pullup_demo_definition(detector=False)
        definition['profile_threshold'] = 1
        report = self.post('api/pullup-quality', {'report': demo['report'], 'definition': definition})
        self.assertEqual(report['quality']['pullup']['profile']['code'], 1)
        self.assertEqual(report['calls'], demo['report']['calls'])
        self.assertEqual(report['peaks'], demo['report']['peaks'])
        definition['profile_threshold'] = 2
        self.assertIn('error', self.post('api/pullup-quality', {'report': report, 'definition': definition}, expected=400))


if __name__ == '__main__':
    unittest.main()
