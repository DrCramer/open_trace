from copy import deepcopy
import unittest
from genemapper_open.pullup_quality import evaluate_pullup_quality,attach_pullup_quality
from genemapper_open.str_demo import make_pullup_demo_definition


def report(flags=(True,),statuses=('tentative',)):
    peaks=[{'id':'DATA1:'+str(i),'scan':10+i,'size_bp':100+i,'height':50,
        'pullup':value,'offscaled':True,'spike':True} for i,value in enumerate(flags)]
    markers=[{'name':'M'+str(i),'status':status,'calls':[],
        'peak_review':[{**p,'peak_index':j,'flags':['pullup'] if p['pullup'] else []} for j,p in enumerate(peaks)]}
        for i,status in enumerate(statuses)]
    return {'calls':[],'peaks':peaks,'allele_calling':{'markers':markers},'quality':{}}


class PullupQualityTests(unittest.TestCase):
    def test_excluded_offscale_spike_flags_still_create_trigger_without_ratio_inference(self):
        q=evaluate_pullup_quality(report(),make_pullup_demo_definition(detector=False))
        trigger=q['markers'][0]['triggers'][0]
        self.assertEqual(q['markers'][0]['metric'],1)
        self.assertEqual((trigger['ratio'],trigger['thresholds']),(-1,[-1]))
        self.assertFalse(trigger['ratio_available']);self.assertEqual(trigger['ordinal'],4)
        self.assertEqual(q['profile']['code'],2)

    def test_multiple_triggers_count_one_marker_and_keep_window_order(self):
        q=evaluate_pullup_quality(report((True,True,False)),make_pullup_demo_definition())
        self.assertEqual([t['source_peak_id'] for t in q['markers'][0]['triggers']],['DATA1:0','DATA1:1'])
        self.assertEqual(q['profile']['positive_markers'],1)

    def test_missing_markers_stay_in_denominator_and_threshold_equality_checks(self):
        r=report(statuses=('tentative','rejected','rejected','tentative'))
        r['allele_calling']['markers'][3]['peak_review']=[]
        d=make_pullup_demo_definition();d['profile_threshold']=.25
        q=evaluate_pullup_quality(r,d)
        self.assertEqual([m['metric'] for m in q['markers']],[1,None,None,0])
        self.assertEqual(q['profile']['code'],1);self.assertEqual(q['profile']['denominator'],4)
        d['profile_threshold']=.249
        self.assertEqual(evaluate_pullup_quality(r,d)['profile']['code'],2)

    def test_empty_initialized_window_is_zero_and_outside_flag_is_not_global_vote(self):
        r=report();r['allele_calling']['markers'][0]['peak_review']=[]
        q=evaluate_pullup_quality(r,make_pullup_demo_definition())
        self.assertEqual(q['profile']['code'],0)
        self.assertEqual(q['markers'][0]['triggers'],[])

    def test_missing_analysis_result_is_unavailable_and_missing_metrics_are_na(self):
        d=make_pullup_demo_definition()
        self.assertIsNone(evaluate_pullup_quality({'allele_calling':None},d)['profile'])
        self.assertEqual(evaluate_pullup_quality(report(statuses=('rejected',)),d)['profile']['code'],-2)
        self.assertEqual(evaluate_pullup_quality(report(statuses=()),d)['profile']['code'],-2)

    def test_retained_measured_ratio_comes_from_detector_state(self):
        r=report();r['peaks'][0]['pullup_classification']={'state':{'ratio':.07,'ratio_threshold':.08},'ratio_provenance':'measured_pair'}
        t=evaluate_pullup_quality(r,make_pullup_demo_definition())['markers'][0]['triggers'][0]
        self.assertEqual(t['ratio'],.07);self.assertEqual(t['thresholds'],[.08])

    def test_attach_does_not_change_calls_gq_or_ladder_admission(self):
        r=report();r['quality']['marker_gq']={'sample_category':0};r['ladder']={'gate_passed':True}
        original=deepcopy(r);q=attach_pullup_quality(r,make_pullup_demo_definition())
        self.assertEqual(r,original)
        self.assertEqual(q['quality']['marker_gq'],r['quality']['marker_gq'])
        self.assertEqual(q['ladder'],r['ladder']);self.assertEqual(q['calls'],r['calls'])
        self.assertFalse(q['quality']['pullup']['gq_or_confidence_changed'])

    def test_consistency_and_duplicate_links_rejected(self):
        r=report();r['allele_calling']['markers'][0]['peak_review']*=2
        with self.assertRaises(ValueError):evaluate_pullup_quality(r,make_pullup_demo_definition())
        r=report();r['allele_calling']['markers'][0]['peak_review'][0]['height']=999
        with self.assertRaises(ValueError):evaluate_pullup_quality(r,make_pullup_demo_definition())


if __name__=='__main__':unittest.main()
