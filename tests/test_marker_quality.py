from copy import deepcopy
import unittest
from genemapper_open.marker_quality import score_ordinary_marker,score_y_marker,WEIGHTS
import json


def settings():
    return {'max_alleles':2,'homozygous_height':150,'heterozygous_height':50,'high_height':1000,'ratio':.5,'width_bp':2}


def call(label='A',height=300,width=1,confidence=.9,offscale=False):
    return {'label':label,'height':height,'fwhm_bp':width,'confidence':confidence,'offscale':offscale}


class MarkerQualityTests(unittest.TestCase):
    def y_score(self,calls,amel_calls):
        return score_y_marker(calls,settings(),dict.fromkeys(WEIGHTS,.5),
            dict.fromkeys(('spike','offscale','overlap','binning'),False),
            sample_sq=.8,amel_calls=amel_calls,control_mismatch=False)

    def test_y_amel_gate_matrix(self):
        for amel,counts in ((None,{0:False,1:True,2:True,3:False}),
                (['X','Y'],{0:False,1:True,2:True,3:False}),
                (['X'],{0:True,1:False,2:False,3:False}),
                ([],{0:False,1:False,2:False,3:False}),
                (['Z'],{0:False,1:False,2:False,3:False})):
            for count,valid in counts.items():
                result=self.y_score([call(str(i)) for i in range(count)],amel)
                self.assertEqual(result['metrics']['allele_number'],0 if valid else 1,(amel,count))

    def test_empty_y_with_amel_x_keeps_gq_and_serializes_missing_mean(self):
        result=self.y_score([],['x'])
        self.assertEqual(result['gq'],1)
        self.assertIsNone(result['confidence_aggregate'])
        self.assertEqual(result['call_confidence_after_sq'],[])
        self.assertTrue(result['events'][0]['original_empty_mean_would_be_nan'])
        json.dumps(result,allow_nan=False)

    def score(self,calls,thresholds=None,weights=None,flags=None,**kwargs):
        return score_ordinary_marker(calls,settings() if thresholds is None else thresholds,
            dict.fromkeys(WEIGHTS,.5) if weights is None else weights,
            dict.fromkeys(('spike','offscale','overlap','binning'),False) if flags is None else flags,
            sample_sq=kwargs.pop('sample_sq',.8),marker_kind=kwargs.pop('marker_kind','autosomal'),
            control_mismatch=kwargs.pop('control_mismatch',False),**kwargs)

    def test_initial_mean_marker_score_and_sample_sq_are_distinct(self):
        result=self.score([call(confidence=.8),call('B',confidence=1)])
        self.assertAlmostEqual(result['gq'],1)
        self.assertAlmostEqual(result['initial_confidence_mean'],.9)
        self.assertEqual(result['call_confidence_before_sq'],[.9,.9])
        self.assertAlmostEqual(result['call_confidence_after_sq'][0],.72)

    def test_too_many_calls_forces_confidence_zero_even_with_partial_gq(self):
        result=self.score([call(),call('B'),call('C')])
        self.assertEqual(result['gq'],.5)
        self.assertEqual(result['initial_confidence_mean'],.9)
        self.assertEqual(result['confidence_aggregate'],0)
        self.assertEqual(result['call_confidence_after_sq'],[0,0,0])

    def test_amel_count_and_missing_x_apply_same_weight_twice(self):
        result=self.score([call('Y'),call('A'),call('B')],marker_kind='amel')
        self.assertEqual(result['gq'],.25)
        self.assertEqual([e['reason'] for e in result['events'] if e.get('criterion')=='allele_number'],
                         ['called_count_above_maximum','amel_without_x'])

    def test_homozygous_threshold_only_at_exactly_one_call(self):
        self.assertEqual(self.score([call(height=100)])['metrics']['absolute_height'],1)
        self.assertEqual(self.score([call(height=100),call('B',height=100)])['metrics']['absolute_height'],0)
        self.assertEqual(self.score([call(height=150)])['metrics']['absolute_height'],0)

    def test_strict_high_ratio_and_width_threshold_equalities(self):
        result=self.score([call(height=1000,width=2),call('B',height=500,width=2)])
        self.assertEqual(result['gq'],1)
        self.assertEqual([result['metrics'][key] for key in ('high_height','ratio','broad')],[0,0,0])

    def test_high_excludes_offscale_and_all_offscale_is_na(self):
        result=self.score([call(height=2000,offscale=True),call('B',height=1000)])
        self.assertEqual(result['metrics']['high_height'],0)
        self.assertEqual(result['metrics']['offscale'],1)
        result=self.score([call(height=2000,offscale=True),call('B',height=2000,offscale=True)])
        self.assertEqual(result['metrics']['high_height'],-2)
        self.assertEqual(result['gq'],.5)

    def test_ratio_uses_all_calls_and_penalties_multiply_once_each(self):
        thresholds=settings();thresholds['max_alleles']=3
        result=self.score([call(height=600),call('B',height=500),call('C',height=100)],thresholds)
        self.assertEqual(result['metrics']['ratio'],1)
        flags=dict.fromkeys(('spike','offscale','overlap','binning'),True)
        result=self.score([call(),call('B')],flags=flags)
        self.assertEqual(result['gq'],.5**4)

    def test_control_failure_and_empty_marker_force_zero(self):
        self.assertEqual(self.score([call()],control_mismatch=True)['gq'],0)
        empty=self.score([])
        self.assertEqual(empty['gq'],0);self.assertEqual(empty['metrics']['allele_number'],1)
        self.assertEqual(empty['call_confidence_after_sq'],[])

    def test_explicit_complete_inputs_required_and_not_mutated(self):
        calls=[call(),call('B')];before=deepcopy(calls);self.score(calls)
        self.assertEqual(calls,before)
        with self.assertRaises(ValueError):self.score(calls,marker_kind='y')
        with self.assertRaises(ValueError):self.score(calls,weights={})
        with self.assertRaises(ValueError):self.score([{'label':'A','height':100}])


if __name__=='__main__':unittest.main()
