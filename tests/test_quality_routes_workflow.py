from copy import deepcopy
import unittest
from genemapper_open.str_demo import (make_str_demo, make_cross_check_demo,
                                    make_negative_demo_definitions, make_quality_demo_definition)
from genemapper_open.str_workflow import analyze_str_files, digest
from genemapper_open.marker_quality_workflow import attach_marker_quality, evaluate_marker_quality
from genemapper_open.control_workflow import attach_control_result
from genemapper_open.project_api import project_action


def analyze(fixture):
    return analyze_str_files(fixture['sample_bytes'],fixture['ladder_bytes'],fixture['kit'],settings=fixture['settings'])


class QualityRoutesWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.report=analyze(make_str_demo())
        fixture,cls.cross_definition=make_cross_check_demo()
        cls.cross_report=analyze(fixture)

    def test_negative_uses_current_controls_without_morphology_or_ordinary_thresholds(self):
        definition,controls=make_negative_demo_definitions()
        report=deepcopy(self.report)
        for peak in report['peaks']:peak.pop('morphology_bp',None)
        report=attach_control_result(report,controls)
        quality=evaluate_marker_quality(report,definition)
        self.assertTrue(quality['complete_for_requested_subset'])
        row=quality['markers'][0]
        self.assertEqual(row['score']['gq'],0)
        self.assertEqual([c['after_sample_sq'] for c in row['call_scores']],[1,1])
        self.assertEqual(report['calls'],self.report['calls'])

    def test_negative_quality_sensor_match_and_spike_weight_then_stale_control(self):
        definition,controls=make_negative_demo_definitions()
        controls['markers']['ExampleM'].update(quality_sensor=True,expected_alleles=['B','C'])
        definition['markers']['ExampleM']['spike_metric']=1
        report=attach_marker_quality(attach_control_result(self.report,controls),definition)
        self.assertEqual(report['quality']['marker_gq']['markers'][0]['score']['gq'],.5)
        self.assertEqual(report['quality']['marker_gq']['sample_category'],1)
        controls['markers']['ExampleM']['expected_alleles']=['B']
        changed=attach_control_result(report,controls)
        self.assertEqual(changed['quality']['marker_gq']['status'],'stale')
        refreshed=attach_marker_quality(changed,definition)
        self.assertEqual(refreshed['quality']['marker_gq']['sample_category'],2)

    def test_missing_stale_or_unperformed_negative_observations_are_unavailable(self):
        definition,controls=make_negative_demo_definitions()
        self.assertEqual(evaluate_marker_quality(self.report,definition)['status'],'unavailable')
        for mode in ('stale','skipped'):
            report=attach_control_result(self.report,controls)
            control=report['quality']['controls']
            if mode=='stale':control['input_observations_sha256']='0'*64
            else:control['markers'][0]['comparison_performed']=False
            self.assertEqual(evaluate_marker_quality(report,definition)['status'],'unavailable')

    def test_changing_sample_to_negative_invalidates_previously_ordinary_gq(self):
        report=attach_marker_quality(self.report,make_quality_demo_definition())
        _,controls=make_negative_demo_definitions()
        changed=attach_control_result(report,controls)
        self.assertEqual(changed['quality']['marker_gq']['status'],'stale')

    def test_actual_synthetic_fsa_to_amel_plugin_keeps_pre_plugin_confidence(self):
        self.assertEqual([(c['marker'],c['label']) for c in self.cross_report['calls']],[('AMEL','X'),('ExampleY','1')])
        quality=evaluate_marker_quality(self.cross_report,self.cross_definition)
        self.assertTrue(quality['complete_for_requested_subset'])
        self.assertTrue(quality['amel_cross_check']['mismatch'])
        amel,y=quality['markers']
        self.assertEqual(amel['score']['gq_before_amel_cc'],1)
        self.assertEqual(amel['score']['gq'],.5)
        self.assertEqual(amel['call_scores'][0]['after_sample_sq'],1)
        self.assertEqual(y['score']['gq_before_amel_cc'],.5)
        self.assertEqual(y['score']['gq'],.25)
        self.assertEqual(y['call_scores'][0]['after_sample_sq'],0)
        self.assertFalse(quality['amel_cross_check']['call_confidences_recomputed'])

    def test_amel_plugin_not_silently_complete_when_y_metrics_or_roles_missing(self):
        report=deepcopy(self.cross_report)
        y_peak=next(c['source_peak_id'] for c in report['calls'] if c['marker']=='ExampleY')
        next(p for p in report['peaks'] if p['id']==y_peak).pop('morphology_bp')
        quality=evaluate_marker_quality(report,self.cross_definition)
        self.assertEqual(quality['amel_cross_check']['status'],'unavailable')
        self.assertFalse(quality['complete_for_requested_subset'])
        self.assertEqual(quality['markers'][0]['score']['gq'],1)
        definition=deepcopy(self.cross_definition);del definition['markers']['ExampleY']
        quality=evaluate_marker_quality(self.cross_report,definition)
        self.assertEqual(quality['amel_cross_check']['reason'],'explicit_panel_marker_roles_missing')

    def test_quality_project_roundtrip_and_repeated_evaluation_are_immutable(self):
        original=digest(self.cross_report)
        report=attach_marker_quality(self.cross_report,self.cross_definition)
        self.assertEqual(digest(self.cross_report),original)
        self.assertEqual(attach_marker_quality(report,self.cross_definition),report)
        state=project_action({'action':'new','name':'Cross-check'})
        state=project_action({'action':'save','document':state['document'],'report':report})
        imported=project_action({'action':'import','document':state['document']})
        self.assertEqual(imported['effective']['machine_result']['quality'],report['quality'])

    def test_schema2_rejects_ambiguous_sample_route_and_implicit_plugin_weight(self):
        definition,_=make_negative_demo_definitions()
        for key,value in (('control_mode','none'),('sample_type',[]),('amel_cross_check',{}),('schema_version',True)):
            mutated=deepcopy(definition);mutated[key]=value
            with self.assertRaises(ValueError):evaluate_marker_quality(self.report,mutated)
        definition['markers']['ExampleM']['thresholds']={}
        with self.assertRaises(ValueError):evaluate_marker_quality(self.report,definition)
