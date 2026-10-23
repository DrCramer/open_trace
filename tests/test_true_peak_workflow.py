from copy import deepcopy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from genemapper_open.server import analyze
from genemapper_open.str_demo import make_true_peak_demo,make_morphology_demo,make_ladder_quality_demo_definition
from genemapper_open.str_workflow import run_str_workflow,compact_analysis
from genemapper_open.project_store import ProjectStore
from genemapper_open.project_runs import create_project_run,revise_project_run


class TruePeakWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture=make_true_peak_demo()
        cls.sample=analyze(cls.fixture['sample_bytes'],'own.fsa',**cls.fixture['settings'])
        cls.ladder=analyze(cls.fixture['ladder_bytes'],'own-ladder.fsa',**cls.fixture['settings'])

    def test_restoration_keeps_reference_sizing_and_recorded_measurements(self):
        baseline=make_morphology_demo()
        self.assertEqual(baseline['sample_bytes'],self.fixture['sample_bytes'])
        before=analyze(baseline['sample_bytes'],'own.fsa',**baseline['settings'])
        self.assertEqual(before['sizing'],self.sample['sizing'])
        reference=lambda report:next(ch for ch in report['channels'] if ch['tag']=='DATA106')
        self.assertEqual(reference(before),reference(self.sample))
        old,new=before['channels'][0]['peaks'][0],self.sample['channels'][0]['peaks'][0]
        for key in ('begin','end','fwhm_scan','raw_height','offscaled','sg_scan','wrapper_source_local'):
            self.assertEqual(old.get(key),new.get(key))
        for key in ('exhalf_width_scan','near_base_width_scan','area_ratio','jag_count','fit_score'):
            self.assertEqual(old['spike_features'][key],new['spike_features'][key])
        self.assertEqual(new['true_peak_restoration']['before']['height'],old['height'])
        self.assertAlmostEqual(new['height'],1012.4836720093544,places=6)
        self.assertEqual(new['scan'],int(new['spike_features']['morphology']['true_peak']['center']))
        self.assertFalse(new['spike']);self.assertEqual(self.sample['artifacts']['restored_peaks'],1)

    def test_exclusion_policy_uses_same_model_without_writes(self):
        fixture=make_true_peak_demo(exclude_overlap=True)
        excluded=analyze(fixture['sample_bytes'],'own.fsa',**fixture['settings'])
        first=excluded['channels'][0]['peaks'][0]
        self.assertEqual(first['true_peak_restoration']['writes'],[])
        self.assertEqual(first['height'],1205)
        self.assertTrue(first['spike'])
        self.assertEqual(first['spike_features']['morphology']['true_peak'],
            self.sample['channels'][0]['peaks'][0]['spike_features']['morphology']['true_peak'])

    def test_changed_height_scan_and_area_reach_sample_calling_and_compact_report(self):
        report=run_str_workflow(self.sample,self.ladder,self.fixture['kit'])
        self.assertEqual([c['label'] for c in report['calls']],['B','C'])
        self.assertAlmostEqual(report['calls'][0]['height'],self.sample['channels'][0]['peaks'][0]['height'])
        self.assertEqual(report['calls'][0]['scan'],self.sample['channels'][0]['peaks'][0]['scan'])
        self.assertTrue(report['provenance']['true_peak_restoration_applied'])
        self.assertEqual(report['peaks'][0]['morphology_bp']['area_bp'],self.sample['channels'][0]['peaks'][0]['area_bp'])
        self.assertNotIn('values',compact_analysis(self.sample)['channels'][0])
        json.dumps(report,allow_nan=False)

    def test_negative_area_sentinel_remains_explicit_without_breaking_calling(self):
        sample=deepcopy(self.sample);p=sample['channels'][0]['peaks'][0]
        p.update(area=-1,area_bp=-.05)
        p['true_peak_restoration']['after'].update(area=-1,area_bp=-.05)
        report=run_str_workflow(sample,self.ladder,self.fixture['kit'])
        self.assertEqual([c['label'] for c in report['calls']],['B','C'])
        self.assertEqual(report['peaks'][0]['area'],-1)
        self.assertEqual(report['peaks'][0]['morphology_bp']['reason'],'restored_negative_area_sentinel')

    def test_negative_map_position_is_unsized_without_fabricating_a_bin(self):
        sample=deepcopy(self.sample);p=sample['channels'][0]['peaks'][0]
        p['size_bp']=-1;p['true_peak_restoration']['after']['size_bp']=-1
        report=run_str_workflow(sample,self.ladder,self.fixture['kit'])
        self.assertEqual([c['label'] for c in report['calls']],['C'])

    def test_rejected_sizing_retains_height_only(self):
        settings={**self.fixture['settings'],'standard_channel':'DATA2'}
        report=analyze(self.fixture['sample_bytes'],'own.fsa',**settings)
        self.assertEqual(report['sizing']['status'],'rejected')
        p=report['channels'][0]['peaks'][0]
        self.assertEqual(p['true_peak_restoration']['status'],'height_only')
        self.assertEqual(p['scan'],3726)
        self.assertAlmostEqual(p['height'],1012.4836720093544,places=6)
        self.assertNotIn('size_bp',p)

    def test_persistent_run_revisions_do_not_apply_restore_again(self):
        inputs={'schema_version':1,'kit':self.fixture['kit'],
            'ladder_quality_definition':make_ladder_quality_demo_definition(),'missing_offset_policy':'reject',
            'ladders':[{'ladder_id':'Own.fsa','analysis':self.ladder}],
            'samples':[{'sample_key':'Own','analysis':self.sample}]}
        with ProjectStore.memory('Restored history') as store:
            created=create_project_run(store,'Run',inputs,actor='Tester',reason='Own fixture')
            old=store.get_analysis(created['outputs'][0]['analysis_id'])
            with patch('genemapper_open.true_peak.restore_peak',side_effect=AssertionError('No second restoration')):
                changed=revise_project_run(store,created['run_id'],created['revision_id'],inputs,actor='Tester',reason='Recalculate calls')
            new=store.get_analysis(changed['outputs'][0]['analysis_id'])
            self.assertEqual(old['result']['peaks'],new['result']['peaks'])
            exported=store.export_json()
        with ProjectStore.from_json(exported) as imported:
            self.assertEqual(imported.get_analysis(changed['outputs'][0]['analysis_id']),new)

    def test_cli_exports_same_own_bytes_and_explicit_recipe(self):
        with tempfile.TemporaryDirectory() as directory:
            target=Path(directory)/'fixture'
            p=subprocess.run([sys.executable,'-m','genemapper_open','str-demo',str(target),'--variant','true-peak'],
                capture_output=True,text=True,timeout=15)
            self.assertEqual(p.returncode,0,p.stderr)
            report=json.loads((target/'report.json').read_text(encoding='utf-8'))
            self.assertEqual([c['label'] for c in report['calls']],['B','C'])
            self.assertTrue(report['provenance']['true_peak_restoration_applied'])
            self.assertEqual((target/'sample.fsa').read_bytes(),self.fixture['sample_bytes'])
            self.assertEqual(json.loads((target/'spike.json').read_text()),self.fixture['settings']['spike_definition'])


if __name__=='__main__':unittest.main()
