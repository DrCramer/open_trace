from copy import deepcopy
import json
import unittest
from genemapper_open.str_demo import make_str_demo
from genemapper_open.str_workflow import analyze_str_files
from genemapper_open.marker_quality import WEIGHTS
from genemapper_open.marker_quality_workflow import evaluate_marker_quality,attach_marker_quality
from genemapper_open.control_workflow import attach_control_result
from genemapper_open.project_api import project_action
from genemapper_open.project_store import ProjectStore


def recipe():
    return {'schema_version':1,'sample_sq':{'source':'base_quality'},'gq_thresholds':{'good':.75,'bad':.25},
        'amel_marker':None,'control_mode':'none','markers':{'ExampleM':{
            'kind':'autosomal','confidence_source':'bin_confidence_weight',
            'thresholds':{'max_alleles':2,'homozygous_height':100,'heterozygous_height':50,
                          'high_height':5000,'ratio':.5,'width_bp':2},
            'weights':dict.fromkeys(WEIGHTS,.5),
            'flags':dict.fromkeys(('spike','offscale','overlap','binning'),False)}}}


def control(expected):
    return {'schema_version':1,'sample_type':'Positive Control','markers':{
        'ExampleM':{'expected_alleles':expected,'quality_sensor':False,'is_y_marker':False}}}


class MarkerQualityWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture=make_str_demo()
        x=cls.fixture
        cls.report=analyze_str_files(x['sample_bytes'],x['ladder_bytes'],x['kit'],settings=x['settings'])

    def test_full_fsa_dimensions_and_initial_bin_confidence_are_available(self):
        result=evaluate_marker_quality(self.report,recipe())
        self.assertEqual(result['status'],'evaluated_subset')
        self.assertTrue(result['complete_for_requested_subset'])
        self.assertFalse(result['full_original_pqv'])
        self.assertEqual(result['sample_category'],0)
        self.assertEqual(result['markers'][0]['score']['gq'],1)
        for peak in self.report['peaks']:
            self.assertAlmostEqual(peak['morphology_bp']['fwhm_bp'],.6,places=8)
        self.assertEqual([s['after_sample_sq'] for s in result['markers'][0]['call_scores']],[1,1])

    def test_bin_confidence_mean_and_sample_sq_multiply_separately(self):
        x=self.fixture;kit=deepcopy(x['kit'])
        kit['markers'][0]['bins'][1]['confidence_weight']=.6
        kit['markers'][0]['bins'][2]['confidence_weight']=.8
        report=analyze_str_files(x['sample_bytes'],x['ladder_bytes'],kit,settings=x['settings'])
        definition=recipe();definition['sample_sq']={'source':'explicit','score':.5}
        result=evaluate_marker_quality(report,definition)['markers'][0]
        self.assertAlmostEqual(result['score']['initial_confidence_mean'],.7)
        self.assertEqual(result['score']['gq'],1)
        self.assertAlmostEqual(result['call_scores'][0]['after_sample_sq'],.35)

    def test_over_count_partial_gq_keeps_call_confidences_zero(self):
        definition=recipe();definition['markers']['ExampleM']['thresholds']['max_alleles']=1
        row=evaluate_marker_quality(self.report,definition)['markers'][0]
        self.assertEqual(row['score']['gq'],.5)
        self.assertEqual(row['category']['code'],1)
        self.assertEqual([v['after_sample_sq'] for v in row['call_scores']],[0,0])

    def test_missing_dimensions_do_not_become_zero_width_or_score(self):
        report=deepcopy(self.report);del report['peaks'][0]['morphology_bp']
        result=evaluate_marker_quality(report,recipe())
        self.assertEqual(result['status'],'unavailable')
        self.assertIsNone(result['sample_category']);self.assertIsNone(result['markers'][0]['score'])
        self.assertFalse(result['all_markers_evaluated'])

    def test_positive_control_change_marks_prior_gq_stale_then_recalculates(self):
        definition=recipe();definition['control_mode']='positive_attached'
        report=attach_control_result(self.report,control(['B','C']))
        report=attach_marker_quality(report,definition)
        self.assertEqual(report['quality']['marker_gq']['sample_category'],0)
        changed=attach_control_result(report,control(['A','B']))
        self.assertEqual(changed['quality']['marker_gq']['status'],'stale')
        self.assertIsNone(changed['quality']['marker_gq']['sample_category'])
        refreshed=attach_marker_quality(changed,definition)
        self.assertEqual(refreshed['quality']['marker_gq']['sample_category'],2)
        self.assertEqual([s['after_sample_sq'] for s in refreshed['quality']['marker_gq']['markers'][0]['call_scores']],[0,0])
        self.assertEqual(refreshed['calls'],self.report['calls'])
        same=attach_control_result(report,control(['B','C']))
        self.assertEqual(same['quality']['marker_gq']['status'],'evaluated_subset')

    def test_missing_expected_control_does_not_mark_subset_complete(self):
        definition=recipe();definition['control_mode']='positive_attached'
        result=evaluate_marker_quality(attach_control_result(self.report,control([])),definition)
        self.assertEqual(result['sample_category'],0)
        self.assertFalse(result['complete_for_requested_subset'])

    def test_explicit_amel_missing_and_negative_control_are_not_ordinary_scores(self):
        definition=recipe();definition['markers']['ExampleM']['kind']='y';definition['amel_marker']='AMEL'
        self.assertEqual(evaluate_marker_quality(self.report,definition)['status'],'unavailable')
        definition=control([]);definition['sample_type']='Negative Control'
        report=attach_control_result(self.report,definition)
        self.assertEqual(evaluate_marker_quality(report,recipe())['markers'][0]['reason'],'negative_control_gq_not_implemented')

    def test_inconsistent_calls_and_invalid_recipes_are_rejected(self):
        report=deepcopy(self.report);report['calls'][0]['label']='changed'
        with self.assertRaises(ValueError):evaluate_marker_quality(report,recipe())
        with self.assertRaises(ValueError):evaluate_marker_quality(self.report,'{"schema_version":1,"schema_version":1}')
        for mutation in ('weights','thresholds'):
            definition=recipe();definition['markers']['ExampleM'][mutation]={}
            with self.assertRaises(ValueError):evaluate_marker_quality(self.report,definition)

    def test_known_off_ladder_and_overlap_are_not_hidden_by_false_recipe_flags(self):
        report=deepcopy(self.report)
        for call in (report['calls'][0],report['allele_calling']['markers'][0]['calls'][0]):
            call.update(label='OL',off_ladder=True);call['flags'].append('cross_marker_overlap')
        row=evaluate_marker_quality(report,recipe())['markers'][0]
        self.assertEqual(row['score']['gq'],.25)
        self.assertTrue(row['flag_sources']['effective']['binning'])
        self.assertTrue(row['flag_sources']['effective']['overlap'])

    def test_quality_project_roundtrip_preserves_measurements_and_machine_calls(self):
        before=deepcopy(self.report);report=attach_marker_quality(self.report,recipe())
        self.assertEqual(before,self.report)
        state=project_action({'action':'new','name':'Marker quality test'})
        state=project_action({'action':'save','document':state['document'],'report':report})
        imported=project_action({'action':'import','document':state['document']})
        saved=imported['effective']['machine_result']
        self.assertEqual(saved['quality'],report['quality']);self.assertEqual(saved['sizing'],report['sizing'])
        self.assertEqual(saved['calls'],report['calls'])
        self.assertNotIn('"values":',json.dumps(saved,allow_nan=False))

    def test_new_project_does_not_reuse_previous_selected_analysis(self):
        state=project_action({'action':'new','name':'First'})
        state=project_action({'action':'save','document':state['document'],'report':self.report})
        new=project_action({'action':'new','name':'New','analysis_id':state['selected_analysis_id']})
        self.assertIsNone(new['selected_analysis_id']);self.assertEqual(new['analyses'],[])

    def test_import_selects_latest_instant_instead_of_sorted_random_id(self):
        with ProjectStore.memory('Selection') as store:
            sample=store.add_sample('a'*64,{})
            for identifier,time in (('z-older','2026-10-21T10:00:00+03:00'),('a-newest','2026-10-21T08:00:00+00:00')):
                store.add_analysis(sample,'a'*64,{'name':'Test','version':'1'}, {},{'calls':[]},'tentative',
                    analysis_id=identifier,created_at=time)
            exported=store.export_json()
        state=project_action({'action':'import','document':exported})
        self.assertEqual(state['selected_analysis_id'],'a-newest')
        self.assertEqual([r['analysis_id'] for r in state['analyses']],['a-newest','z-older'])


if __name__=='__main__':unittest.main()
