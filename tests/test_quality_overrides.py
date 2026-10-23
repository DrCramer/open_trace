"""Manual GQ/CGQ overrides: forced values, freshness and undo."""

from copy import deepcopy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from genemapper_open.marker_quality_workflow import attach_marker_quality
from genemapper_open.project_api import project_action
from genemapper_open.project_store import ProjectStore, ProjectStoreError
from genemapper_open.server import analyze
from genemapper_open.str_demo import (make_pullup_demo, make_ladder_quality_demo_definition,
                                      make_quality_demo_definition)
from genemapper_open.str_workflow import run_str_workflow

ACTOR = {'actor': 'Tester', 'reason': 'Own synthetic fixture'}
ROOT = Path(__file__).resolve().parents[1]


def demo_report():
    fixture = make_pullup_demo()
    sample = analyze(fixture['sample_bytes'], 'sample.fsa', **fixture['settings'])
    ladder = analyze(fixture['ladder_bytes'], 'ladder.fsa', **fixture['settings'])
    report = run_str_workflow(sample, ladder, fixture['kit'],
                              ladder_quality_definition=make_ladder_quality_demo_definition())
    return attach_marker_quality(report, make_quality_demo_definition())


class QualityOverrideTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.base = demo_report()
        cls.report = deepcopy(cls.base)
        # Lower the machine GQ in this stored copy so the forced value is visible.
        cls.report['quality']['marker_gq']['markers'][0]['score']['gq'] = 0.4
        created = project_action({'action': 'new', 'name': 'Overrides'})
        cls.saved = project_action({'action': 'save', 'document': created['document'], 'report': cls.report})

    def setUp(self):
        self.store = ProjectStore.from_json(self.saved['document'])
        self.analysis = self.saved['selected_analysis_id']

    def tearDown(self):
        self.store.close()

    def review(self, action, kind, target=None, value=None, analysis=None):
        return self.store.append_review(analysis or self.analysis, action, target_kind=kind,
                                        target_id=target, value=value, **ACTOR)

    def quality(self):
        return self.store.effective_analysis(self.analysis)['review_quality']

    def marker(self, quality):
        return next(row for row in quality['markers'] if row['marker'] == 'ExampleM')

    def test_override_gq_forces_one_marks_marker_stale_only(self):
        before = self.store.get_analysis(self.analysis)
        event = self.review('override-gq', 'marker', 'ExampleM')
        effective = self.store.effective_analysis(self.analysis)
        quality = effective['review_quality']
        row = self.marker(quality)
        self.assertEqual(row['gq_override'], {'gq': 1.0, 'override': True, 'event_id': event, 'machine_gq': 0.4})
        self.assertEqual(row['freshness'], 'stale')
        self.assertEqual(row['active_event_ids'], [event])
        self.assertEqual(quality['affected_markers'], ['ExampleM'])
        self.assertEqual(quality['sample_summary']['freshness'], 'unchanged')
        self.assertNotIn('cgq_override', quality['sample_summary'])
        self.assertEqual(effective['review_notes'], [])
        self.assertEqual(self.store.get_analysis(self.analysis), before)

    def test_undo_override_gq_restores_machine_state(self):
        event = self.review('override-gq', 'marker', 'ExampleM')
        self.store.undo_review(self.analysis, event, **ACTOR)
        row = self.marker(self.quality())
        self.assertNotIn('gq_override', row)
        self.assertEqual(row['freshness'], 'unchanged')
        again = self.review('override-gq', 'marker', 'ExampleM')
        self.assertEqual(self.marker(self.quality())['gq_override']['event_id'], again)

    def test_override_gq_rejections(self):
        call = self.report['calls'][0]['id']
        cases = (('unknown marker', ('override-gq', 'marker', 'Missing')),
                 ('call target', ('override-gq', 'call', call)),
                 ('analysis target', ('override-gq', 'analysis', None)),
                 ('value given', ('override-gq', 'marker', 'ExampleM', '0.9')),
                 ('note on marker', ('note', 'marker', 'ExampleM', 'text')),
                 ('cgq on marker', ('override-cgq', 'marker', 'ExampleM')),
                 ('cgq on peak', ('override-cgq', 'peak', 'DATA1:0')))
        for name, arguments in cases:
            with self.subTest(name):
                with self.assertRaises(ProjectStoreError):
                    self.review(*arguments)
        self.review('override-gq', 'marker', 'ExampleM')
        with self.assertRaises(ProjectStoreError):
            self.review('override-gq', 'marker', 'ExampleM')
        self.assertEqual(len(self.store.review_events(self.analysis)), 1)

    def test_override_cgq_writes_half_as_override_pass(self):
        event = self.review('override-cgq', 'analysis')
        effective = self.store.effective_analysis(self.analysis)
        summary = effective['review_quality']['sample_summary']
        self.assertEqual(summary['cgq_override'], {'cgq': 0.5, 'category': 'override_pass', 'override': True,
                                                   'event_id': event, 'machine_category': 0})
        self.assertEqual(summary['freshness'], 'unchanged')
        self.assertEqual(effective['review_notes'], [])
        self.assertTrue(all('gq_override' not in row for row in effective['review_quality']['markers']))
        with self.assertRaises(ProjectStoreError):
            self.review('override-cgq', 'analysis')
        self.store.undo_review(self.analysis, event, **ACTOR)
        self.assertNotIn('cgq_override', self.quality()['sample_summary'])

    def test_overrides_require_machine_gq(self):
        plain = deepcopy(self.report); plain['quality'].pop('marker_gq')
        sample = self.store.list_samples()[0]['sample_id']
        other = self.store.add_analysis(sample, plain['source_sha256'], {'name': 'copy', 'version': 'test'},
                                        {}, {k: plain[k] for k in ('calls', 'peaks', 'quality', 'allele_calling')},
                                        'tentative')
        for arguments in (('override-gq', 'marker', 'ExampleM'), ('override-cgq', 'analysis')):
            with self.subTest(arguments[0]):
                with self.assertRaises(ProjectStoreError):
                    self.review(*arguments, analysis=other)

    def test_override_survives_later_peak_edit_and_sample_becomes_stale(self):
        override = self.review('override-gq', 'marker', 'ExampleM')
        added = self.review('add-allele', 'peak', 'DATA1:0', 'B')
        quality = self.quality()
        row = self.marker(quality)
        self.assertEqual(row['gq_override']['event_id'], override)
        self.assertEqual(row['active_event_ids'], [override, added])
        self.assertEqual(quality['sample_summary']['freshness'], 'stale')

    def test_export_import_replays_overrides(self):
        gq = self.review('override-gq', 'marker', 'ExampleM')
        self.review('override-cgq', 'analysis')
        self.store.undo_review(self.analysis, gq, **ACTOR)
        expected = self.store.effective_analysis(self.analysis)
        with ProjectStore.from_json(self.store.export_json()) as imported:
            self.assertEqual(imported.effective_analysis(self.analysis), expected)

    def test_unmodified_machine_gq_is_still_reported(self):
        self.assertEqual(self.base['quality']['marker_gq']['markers'][0]['score']['gq'], 1.0)
        event = self.review('override-gq', 'marker', 'ExampleM')
        self.assertEqual(self.marker(self.quality())['gq_override']['event_id'], event)


class QualityOverrideCliTests(unittest.TestCase):
    def command(self, *args):
        process = subprocess.run([sys.executable, '-m', 'genemapper_open', *map(str, args)], cwd=ROOT,
                                 capture_output=True, text=True, encoding='utf-8', timeout=30)
        self.assertEqual(process.returncode, 0, process.stderr)
        return process.stdout

    def test_cli_overrides_and_undo(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); report = root/'report.json'; db = root/'project.sqlite'
            report.write_text(json.dumps(demo_report(), allow_nan=False), encoding='utf-8')
            self.command('project', 'create', db, '--name', 'CLI overrides')
            analysis = json.loads(self.command('project', 'save', db, report))['analysis_id']
            gq = json.loads(self.command('project', 'review', db, analysis, 'override-gq', '--target-kind', 'marker',
                '--target-id', 'ExampleM', '--actor', 'Test', '--reason', 'Reviewed'))
            row = next(r for r in gq['effective']['review_quality']['markers'] if r['marker'] == 'ExampleM')
            self.assertEqual(row['gq_override']['gq'], 1.0)
            cgq = json.loads(self.command('project', 'review', db, analysis, 'override-cgq',
                '--actor', 'Test', '--reason', 'Reviewed'))
            self.assertEqual(cgq['effective']['review_quality']['sample_summary']['cgq_override']['cgq'], 0.5)
            undone = json.loads(self.command('project', 'undo', db, analysis, gq['event_id'],
                '--actor', 'Test', '--reason', 'Restore'))
            row = next(r for r in undone['effective']['review_quality']['markers'] if r['marker'] == 'ExampleM')
            self.assertNotIn('gq_override', row)


if __name__ == '__main__':
    unittest.main()
