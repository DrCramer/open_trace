from copy import deepcopy
import json
import unittest
from genemapper_open.str_demo import make_str_demo,make_spike_demo_definition,make_quality_demo_definition,make_ladder_quality_demo_definition
from genemapper_open.str_workflow import analyze_str_files,run_str_workflow,compact_analysis
from genemapper_open.server import analyze
from genemapper_open.marker_quality_workflow import evaluate_marker_quality
from genemapper_open.project_api import project_action


class SpikeWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture=make_str_demo(include_spike=True);x=cls.fixture
        cls.settings=x['settings']|{'spike_definition':make_spike_demo_definition()}
        cls.sample=analyze(x['sample_bytes'],'sample.fsa',**cls.settings)
        cls.ladder=analyze(x['ladder_bytes'],'ladder.fsa',**cls.settings)
        cls.report=run_str_workflow(cls.sample,cls.ladder,x['kit'])

    def test_generated_narrow_peak_is_automatically_excluded_before_calling(self):
        self.assertEqual(self.sample['artifacts']['current_spikes'],1)
        self.assertEqual([c['label'] for c in self.report['calls']],['B','C'])
        spike=next(p for p in self.report['peaks'] if p.get('spike'))
        self.assertTrue(spike['spike_classification']['spike_detected'])
        self.assertLess(spike['spike_features']['near_base_width_scan'],10)
        self.assertEqual(self.report['allele_calling']['markers'][0]['peak_review'][0]['state'],'excluded_artifact')
        self.assertTrue(self.report['provenance']['artifact_detectors_applied'])

    def test_same_fsa_without_classifier_keeps_additional_a_call(self):
        x=self.fixture
        unrequested=analyze_str_files(x['sample_bytes'],x['ladder_bytes'],x['kit'],settings=x['settings'])
        self.assertEqual([c['label'] for c in unrequested['calls']],['A','B','C'])
        self.assertFalse(unrequested['provenance']['artifact_detectors_applied'])

    def test_original_sizing_reference_is_unchanged(self):
        x=self.fixture;plain=analyze(x['sample_bytes'],'sample.fsa',**x['settings'])
        self.assertEqual(plain['sizing'],self.sample['sizing'])
        reference=next(c for c in self.sample['channels'] if c['tag']=='DATA106')
        self.assertFalse(any('spike_classification' in p for p in reference['peaks']))

    def test_known_automatic_spike_reaches_marker_gq_even_with_false_recipe(self):
        quality=evaluate_marker_quality(self.report,make_quality_demo_definition())
        row=quality['markers'][0]
        self.assertTrue(row['flag_sources']['effective']['spike'])
        self.assertEqual(row['score']['gq'],.5)

    def test_negative_sensor_spike_metric_cannot_be_hidden_by_false_recipe(self):
        from genemapper_open.str_demo import make_negative_demo_definitions
        from genemapper_open.control_workflow import attach_control_result
        definition,controls=make_negative_demo_definitions()
        controls['markers']['ExampleM'].update(quality_sensor=True,expected_alleles=['B','C'])
        report=attach_control_result(self.report,controls)
        quality=evaluate_marker_quality(report,definition)
        self.assertEqual(quality['markers'][0]['score']['metrics']['spike'],1)
        self.assertEqual(quality['markers'][0]['score']['gq'],.5)

    def test_bp_units_and_missing_calibrated_bounds(self):
        x=self.fixture;definition=make_spike_demo_definition()|{'units':'bp','near_base_width_threshold':.5,'exhalf_width_threshold':.3}
        settings=x['settings']|{'spike_definition':definition}
        result=analyze_str_files(x['sample_bytes'],x['ladder_bytes'],x['kit'],settings=settings)
        self.assertEqual([c['label'] for c in result['calls']],['B','C'])
        sample=deepcopy(self.sample);sample['channels'][0]['peaks'][0]['spike_classification']['status']='unavailable'
        rejected=run_str_workflow(sample,self.ladder,x['kit'])
        self.assertEqual(rejected['status'],'rejected');self.assertEqual(rejected['calls'],[])

    def test_compact_and_project_preserve_feature_provenance_and_flags(self):
        x=self.fixture
        compact=run_str_workflow(compact_analysis(self.sample),compact_analysis(self.ladder),x['kit'])
        self.assertEqual(compact,self.report)
        state=project_action({'action':'new','name':'Automatic initial spike'})
        state=project_action({'action':'save','document':state['document'],'report':self.report})
        imported=project_action({'action':'import','document':state['document']})
        self.assertEqual(imported['effective']['machine_result']['peaks'],self.report['peaks'])
        self.assertNotIn('"values":',json.dumps(self.report,allow_nan=False))

    def test_different_requested_settings_and_unsupported_engine_rejected(self):
        sample=deepcopy(self.sample);sample['peak_settings']['spike_definition']['min_height']=101
        self.assertEqual(run_str_workflow(sample,self.ladder,self.fixture['kit'])['status'],'rejected')
        with self.assertRaises(ValueError):analyze(self.fixture['sample_bytes'],'sample.fsa',spike_definition=make_spike_demo_definition())
        with self.assertRaises(ValueError):analyze(self.fixture['sample_bytes'],'sample.fsa',
            analysis_engine='morphology_ratio',spike_definition=make_spike_demo_definition())
