"""Peak edits: user allele on artifact-excluded peaks and artifact-label deletion."""

from copy import deepcopy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from genemapper_open.project_api import project_action
from genemapper_open.project_store import ProjectStore, ProjectStoreError, EDIT_POLICY
from genemapper_open.server import analyze
from genemapper_open.str_demo import make_pullup_demo, make_ladder_quality_demo_definition
from genemapper_open.str_workflow import run_str_workflow

ACTOR = {'actor': 'Tester', 'reason': 'Own synthetic fixture'}
ROOT = Path(__file__).resolve().parents[1]


class PeakEditTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        fixture = make_pullup_demo()
        sample = analyze(fixture['sample_bytes'], 'sample.fsa', **fixture['settings'])
        ladder = analyze(fixture['ladder_bytes'], 'ladder.fsa', **fixture['settings'])
        cls.report = run_str_workflow(sample, ladder, fixture['kit'],
                                      ladder_quality_definition=make_ladder_quality_demo_definition())
        created = project_action({'action': 'new', 'name': 'Peak edits'})
        cls.saved = project_action({'action': 'save', 'document': created['document'], 'report': cls.report})

    def setUp(self):
        self.store = ProjectStore.from_json(self.saved['document'])
        self.analysis = self.saved['selected_analysis_id']

    def tearDown(self):
        self.store.close()

    def edit(self, action, target='DATA1:0', value=None, kind='peak'):
        return self.store.append_review(self.analysis, action, target_kind=kind, target_id=target,
                                        value=value, **ACTOR)

    def peak(self, effective, identifier='DATA1:0'):
        return next(t for t in effective['review_targets']
                    if t['target_kind'] == 'peak' and t['target_id'] == identifier)

    def test_fixture_has_pullup_excluded_peak_and_called_peak(self):
        self.assertEqual([c['label'] for c in self.report['calls']], ['C'])
        effective = self.store.effective_analysis(self.analysis)
        excluded = self.peak(effective)
        self.assertEqual(excluded['marker'], 'ExampleM')
        self.assertEqual(excluded['machine_artifact_flags'], {'pullup': True, 'spike': False})
        self.assertEqual(excluded['artifact_label'], 'SPU')
        self.assertEqual(self.peak(effective, 'DATA1:1')['machine_artifact_label'], None)
        self.assertEqual(effective['user_calls'], [])
        self.assertEqual(effective['edit_policy'], EDIT_POLICY)
        self.assertEqual(effective['review_quality']['sample_summary']['freshness'], 'unchanged')
        self.assertEqual(effective['review_quality']['pullup_profile'],
                         {'freshness': 'unchanged', 'active_event_ids': []})

    def test_add_allele_clears_flags_marks_stale_and_keeps_machine_result(self):
        before = self.store.get_analysis(self.analysis)
        event = self.edit('add-allele', value='B')
        effective = self.store.effective_analysis(self.analysis)
        peak = self.peak(effective)
        self.assertEqual(peak['user_allele'], {'label': 'B', 'call_type': 'USER', 'marker': 'ExampleM',
                                               'event_id': event, 'actor': 'Tester'})
        self.assertEqual(peak['artifact_flags'], {'pullup': False, 'spike': False})
        self.assertEqual(peak['machine_artifact_flags'], {'pullup': True, 'spike': False})
        self.assertIsNone(peak['artifact_label'])
        self.assertEqual(effective['user_calls'], [{'peak_id': 'DATA1:0', **peak['user_allele']}])
        quality = effective['review_quality']
        self.assertEqual(quality['affected_markers'], ['ExampleM'])
        self.assertEqual(quality['active_event_ids'], [event])
        self.assertEqual(quality['sample_summary']['freshness'], 'stale')
        self.assertEqual(quality['pullup_profile'], {'freshness': 'stale', 'active_event_ids': [event]})
        self.assertEqual(self.store.get_analysis(self.analysis), before)
        self.assertEqual(effective['machine_result']['peaks'][0]['pullup'], True)

    def test_undo_add_allele_restores_every_machine_flag(self):
        event = self.edit('add-allele', value='B')
        self.store.undo_review(self.analysis, event, **ACTOR)
        effective = self.store.effective_analysis(self.analysis)
        peak = self.peak(effective)
        self.assertIsNone(peak['user_allele'])
        self.assertEqual(peak['artifact_flags'], peak['machine_artifact_flags'])
        self.assertEqual(peak['artifact_label'], 'SPU')
        self.assertEqual(effective['user_calls'], [])
        self.assertEqual(effective['review_quality']['sample_summary']['freshness'], 'unchanged')
        self.assertEqual(effective['review_quality']['pullup_profile']['freshness'], 'unchanged')
        again = self.edit('add-allele', value='B2')
        self.assertEqual(self.peak(self.store.effective_analysis(self.analysis))['user_allele']['event_id'], again)

    def test_add_allele_rejections(self):
        cases = (
            ('called peak', dict(target='DATA1:1', value='X')),
            ('blank label', dict(value='  ')),
            ('missing label', dict(value=None)),
            ('long label', dict(value='X' * 257)),
            ('call target', dict(kind='call', target=self.report['calls'][0]['id'], value='X')),
            ('unknown peak', dict(target='DATA9:0', value='X')),
        )
        for name, options in cases:
            with self.subTest(name):
                with self.assertRaises(ProjectStoreError):
                    self.edit('add-allele', **options)
        self.edit('add-allele', value='X' * 256)
        with self.assertRaises(ProjectStoreError):
            self.edit('add-allele', value='Y')
        self.assertEqual(len(self.store.review_events(self.analysis)), 1)

    def test_unlinked_peak_cannot_receive_user_allele(self):
        result = deepcopy(self.store.get_analysis(self.analysis)['result'])
        result['allele_calling']['markers'][0]['peak_review'] = [
            entry for entry in result['allele_calling']['markers'][0]['peak_review'] if entry['peak_index'] != 0]
        sample = self.store.list_samples()[0]['sample_id']
        other = self.store.add_analysis(sample, self.report['source_sha256'], {'name': 'copy', 'version': 'test'}, {}, result, 'tentative')
        with self.assertRaises(ProjectStoreError):
            self.store.append_review(other, 'add-allele', target_kind='peak', target_id='DATA1:0', value='B', **ACTOR)

    def test_delete_artifact_label_hides_label_keeps_flags_and_is_not_undoable(self):
        event = self.edit('delete-artifact-label')
        effective = self.store.effective_analysis(self.analysis)
        peak = self.peak(effective)
        self.assertEqual(peak['artifact_label'], '')
        self.assertEqual(peak['artifact_flags'], {'pullup': True, 'spike': False})
        self.assertEqual(effective['review_quality']['affected_markers'], ['ExampleM'])
        self.assertEqual(effective['review_quality']['pullup_profile']['freshness'], 'unchanged')
        with self.assertRaises(ProjectStoreError):
            self.store.undo_review(self.analysis, event, **ACTOR)
        with self.assertRaises(ProjectStoreError):
            self.edit('delete-artifact-label')
        with self.assertRaises(ProjectStoreError):
            self.edit('delete-artifact-label', target='DATA1:1')
        with self.assertRaises(ProjectStoreError):
            self.edit('delete-artifact-label', value='x')
        added = self.edit('add-allele', value='B')
        peak = self.peak(self.store.effective_analysis(self.analysis))
        self.assertEqual((peak['user_allele']['event_id'], peak['artifact_flags']['pullup']), (added, False))

    def test_delete_artifact_label_after_user_allele_is_rejected(self):
        self.edit('add-allele', value='B')
        with self.assertRaises(ProjectStoreError):
            self.edit('delete-artifact-label')

    def test_export_import_preserves_folded_peak_edits(self):
        self.edit('delete-artifact-label')
        event = self.edit('add-allele', value='B')
        expected = self.store.effective_analysis(self.analysis)
        with ProjectStore.from_json(self.store.export_json()) as imported:
            self.assertEqual(imported.effective_analysis(self.analysis), expected)
            imported.undo_review(self.analysis, event, **ACTOR)
            peak = self.peak(imported.effective_analysis(self.analysis))
            self.assertEqual((peak['artifact_label'], peak['artifact_flags']['pullup']), ('', True))

    def test_browser_project_action_routes_peak_edits(self):
        state = project_action({'action': 'review', 'document': self.saved['document'],
                                'analysis_id': self.analysis, 'review_action': 'add-allele',
                                'target_kind': 'peak', 'target_id': 'DATA1:0', 'value': 'B', **ACTOR})
        self.assertEqual(state['effective']['user_calls'][0]['label'], 'B')
        event = state['events'][-1]['event_id']
        state = project_action({'action': 'undo', 'document': state['document'],
                                'analysis_id': self.analysis, 'event_id': event, **ACTOR})
        self.assertEqual(state['effective']['user_calls'], [])
        json.dumps(state, allow_nan=False)


class PeakEditCliTests(unittest.TestCase):
    def command(self, *args):
        process = subprocess.run([sys.executable, '-m', 'genemapper_open', *map(str, args)], cwd=ROOT,
                                 capture_output=True, text=True, encoding='utf-8', timeout=30)
        self.assertEqual(process.returncode, 0, process.stderr)
        return process.stdout

    def test_cli_add_allele_on_pullup_peak_and_undo(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); fixture = root/'fixture'; db = root/'project.sqlite'
            self.command('str-demo', fixture, '--variant', 'pullup')
            self.command('project', 'create', db, '--name', 'CLI peak edit')
            analysis = json.loads(self.command('project', 'save', db, fixture/'report.json'))['analysis_id']
            added = json.loads(self.command('project', 'review', db, analysis, 'add-allele', '--target-kind', 'peak',
                '--target-id', 'DATA1:0', '--value', 'B', '--actor', 'Test', '--reason', 'Own fixture'))
            peak = next(t for t in added['effective']['review_targets'] if t['target_id'] == 'DATA1:0')
            self.assertEqual((peak['user_allele']['label'], peak['artifact_flags']['pullup']), ('B', False))
            undone = json.loads(self.command('project', 'undo', db, analysis, added['event_id'],
                '--actor', 'Test', '--reason', 'Restore'))
            self.assertEqual(undone['effective']['user_calls'], [])
            peak = next(t for t in undone['effective']['review_targets'] if t['target_id'] == 'DATA1:0')
            self.assertEqual((peak['artifact_flags']['pullup'], peak['artifact_label']), (True, 'SPU'))


if __name__ == '__main__':
    unittest.main()
