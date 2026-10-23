from copy import deepcopy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from genemapper_open.server import analyze
from genemapper_open.str_demo import make_morphology_demo, make_spike_demo_definition
from genemapper_open.str_workflow import analyze_str_files, compact_analysis, run_str_workflow
from genemapper_open.project_store import ProjectStore
from genemapper_open.run_workflow import run_str_run_workflow
from genemapper_open.str_demo import make_ladder_quality_demo_definition


class MorphologyWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture=make_morphology_demo()
        cls.sample=analyze(cls.fixture['sample_bytes'],'own-sample.fsa',**cls.fixture['settings'])
        cls.ladder=analyze(cls.fixture['ladder_bytes'],'own-ladder.fsa',**cls.fixture['settings'])

    def test_companion_route_changes_artifact_flags_preserves_sizing_and_ordinary_dimensions(self):
        plain=make_morphology_demo(enabled=False)
        self.assertEqual(plain['sample_bytes'],self.fixture['sample_bytes'])
        previous=analyze(plain['sample_bytes'],'own-sample.fsa',**plain['settings'])
        self.assertEqual(previous['sizing'],self.sample['sizing'])
        reference=lambda report:next(c for c in report['channels'] if c['tag']=='DATA106')
        self.assertEqual(reference(previous),reference(self.sample))
        old,new=previous['channels'][0]['peaks'],self.sample['channels'][0]['peaks']
        for a,b in zip(old,new):
            for key in ('scan','begin','end','height','area','fwhm_scan','size_bp'):
                self.assertEqual(a[key],b[key])
        self.assertEqual([p['spike_features']['jag_count'] for p in new],[1,0])
        self.assertEqual(new[0]['spike_features']['raw_height'],1600)
        self.assertLess(new[0]['height'],1600) # smoothed ordinary height remains separate
        self.assertEqual(new[0]['spike_classification']['peak_deformity'],'poor_fit')
        self.assertFalse(new[0]['spike_classification']['spike_detected'])
        self.assertTrue(new[0]['spike_classification']['current_spike'])

    def test_schema_one_prepared_measurement_is_preserved(self):
        settings=deepcopy(self.fixture['settings']);settings['spike_definition']=make_spike_demo_definition()
        basic=analyze(self.fixture['sample_bytes'],'own-sample.fsa',**settings)
        self.assertEqual(basic['artifacts']['current_spikes'],0)
        self.assertEqual(basic['channels'][0]['peaks'][0]['spike_features']['measurement_source'],'prepared_signal')
        self.assertEqual(basic['channels'][0]['peaks'][0]['spike_features']['jag_count'],-1)

    def test_calling_and_project_roundtrip_preserve_measurements_and_deformity(self):
        report=run_str_workflow(self.sample,self.ladder,self.fixture['kit'])
        self.assertEqual([c['label'] for c in report['calls']],['C'])
        self.assertEqual(report['peaks'][0]['spike_features']['jag_count'],1)
        self.assertEqual(report['peaks'][0]['spike_classification']['peak_deformity'],'poor_fit')
        self.assertFalse(report['provenance']['true_peak_restoration_applied'])
        with ProjectStore.memory('Morphology') as project:
            source=report['source_sha256']
            sample_id=project.add_sample(source,{'kind':'own fixture'})
            saved=project.add_analysis(sample_id,source,{'name':'OpenTrace','version':report['analysis_version']},
                report['settings'],report,report['status'])
            document=project.export_json()
        with ProjectStore.from_json(document) as imported:
            self.assertEqual(imported.get_analysis(saved)['result'],report)
        compact=compact_analysis(self.sample)
        self.assertNotIn('values',compact['channels'][0])
        json.dumps(compact,allow_nan=False)

    def test_run_workflow_retains_same_measured_sample_without_reclassification(self):
        report=run_str_run_workflow(self.sample,[{'ladder_id':'Own.fsa','analysis':self.ladder}],
            self.fixture['kit'],ladder_quality_definition=make_ladder_quality_demo_definition())
        self.assertEqual([c['label'] for c in report['calls']],['C'])
        self.assertEqual(report['peaks'][0]['spike_features'],self.sample['channels'][0]['peaks'][0]['spike_features'])
        self.assertEqual(report['provenance']['morphology_scope'],'companion_jag_and_ideal_gaussian_fit_only')

    def test_cli_fixture_and_raw_fsa_analysis_use_same_definition(self):
        with tempfile.TemporaryDirectory() as directory:
            target=Path(directory)/'own'
            process=subprocess.run([sys.executable,'-m','genemapper_open','str-demo',str(target),'--variant','morphology'],
                capture_output=True,text=True,timeout=15)
            self.assertEqual(process.returncode,0,process.stderr)
            report=json.loads((target/'report.json').read_text(encoding='utf-8'))
            self.assertEqual([c['label'] for c in report['calls']],['C'])
            self.assertEqual(json.loads((target/'spike.json').read_text()),self.fixture['settings']['spike_definition'])
            self.assertEqual((target/'sample.fsa').read_bytes(),self.fixture['sample_bytes'])


if __name__=='__main__':unittest.main()
