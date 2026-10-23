from copy import deepcopy
import json
import math
import unittest

from genemapper_open.peak_morphology import jag_metrics, gaussian_samples, gaussian_fit_score, _extract
from genemapper_open.spike import load_spike_definition, measure_spike_features, measure_spike_peaks, classify_spike
from genemapper_open.str_demo import make_spike_demo_definition


def definition():
    return make_spike_demo_definition() | {'schema_version':3,
        'measurement_source':'baseline_corrected_companion','morphology_enabled':True,
        'morphology':{'jag_threshold':.4,'fit_score_threshold':.9},
        'adjustments':{'sample_cutoff':False,'sample_marker_envelope':False,'ladder_height_ratio':0}}


class PeakMorphologyTests(unittest.TestCase):
    def test_hand_computed_jag_denominator_count_and_strict_threshold(self):
        measured=jag_metrics([2,6,10,6,2],.4)
        self.assertEqual(measured,{'score':.08,'count':0,'maximum':.4})
        self.assertEqual(jag_metrics([2,6,10,6,2],.399)['count'],1)
        self.assertEqual(jag_metrics([1,1,1],0),{'score':0,'count':0,'maximum':0})

    def test_uncomputed_jag_is_not_zero(self):
        for values in ([],[2,3],[0,0,0],[-1,-2,-1]):
            self.assertEqual(jag_metrics(values,0),{'score':-1,'count':-1,'maximum':-1})

    def test_ideal_gaussian_and_anticorrelation_both_score_one(self):
        values=[10*math.exp(-((i-15)**2)/2) for i in range(31)]
        self.assertAlmostEqual(gaussian_fit_score(values,2.35482,10,15),1)
        # Squaring intentionally loses the sign of the correlation.
        self.assertAlmostEqual(gaussian_fit_score([10-v for v in values],2.35482,10,15),1)

    def test_exact_constant_variance_branch_is_one(self):
        self.assertEqual(gaussian_fit_score([0]*31,3,10,15),1)
        self.assertEqual(gaussian_fit_score([2,6,3],3,0,1),1)

    def test_resampler_target_is_not_an_output_count(self):
        for count,expected in ((2,29),(15,29),(16,28),(29,29),(30,30),(31,31)):
            a,b=gaussian_samples(list(range(count)),4,10,1)
            self.assertEqual((len(a),len(b)),(expected,expected))
            self.assertEqual(b[:count],list(range(count)))
        a,b=gaussian_samples(list(range(16)),4,10,1)
        self.assertAlmostEqual(b[16],1.11)
        self.assertAlmostEqual(b[17],2.22)
        self.assertLess(b[16],b[15]) # appended positions are not sorted

    def test_inclusive_slices_and_bottom_top_fallback(self):
        x=[0,2,6,10,6,2,0]
        whole,top,left,right,selection=_extract(x,0,6,3,.2,-1)
        self.assertEqual((whole,top,left,right),([2,6,10,6,2],[2,6,10,6,2],[2],[2]))
        self.assertEqual(selection['relative_center'],2)
        self.assertEqual(selection['origin'],1)
        measured=measure_spike_features(x,{'begin':0,'end':6,'sg_scan':3},definition()|{'near_base_fraction':.2})
        self.assertEqual(measured['jag_count'],0)
        self.assertEqual(measured['morphology']['jag_score_bottom'],.08)
        self.assertAlmostEqual(measured['area_ratio'],25/26)

    def test_only_left_truncated_mirrors_right_and_origin(self):
        whole,top,left,right,selection=_extract([6,10,6,2,0],0,4,1,.2,-1)
        self.assertEqual(whole,[2,6,10,6,2])
        self.assertEqual((selection['origin'],selection['relative_center']),(-1,2))
        self.assertEqual(selection['truncated'],[True,False])
        self.assertEqual(left,list(reversed(right)))

    def test_only_right_truncated_mirrors_left(self):
        whole,top,left,right,selection=_extract([0,2,6,10,6],0,4,3,.2,-1)
        self.assertEqual(whole,[2,6,10,6,2])
        self.assertEqual(selection['truncated'],[False,True])
        self.assertEqual(right,list(reversed(left)))

    def test_double_truncation_retains_asymmetry_and_original_support(self):
        whole,top,left,right,selection=_extract([6,10,7,3],0,3,1,.1,-1)
        self.assertEqual(whole,[6,10,7,3])
        self.assertEqual(selection['truncated'],[True,True])

    def test_near_threshold_above_half_uses_outer_bounds_for_bottom(self):
        whole,top,left,right,_=_extract([0,1,3,6,10,6,3,1,0],0,8,4,.7,-1)
        self.assertEqual(whole,[6,10,6])
        self.assertEqual(top,[3,6,10,6,3])
        self.assertEqual(left,[0,1,3])
        self.assertEqual(right,[3,1,0])

    def test_max_tie_retains_initial_distance_and_boundary_falls_back(self):
        # First max1 is3 away from c4; later3 then6 are both closer than3.
        _,_,_,_,s=_extract([0,10,2,10,2,2,10,0],0,7,4,.1,-1)
        self.assertEqual(s['center'],6)
        _,_,_,_,s=_extract([20,2,8,2,0],0,4,2,.1,-1)
        self.assertEqual((s['center'],s['height']),(2,8))

    def test_fraction_one_crossing_starts_outside_center(self):
        _,_,_,_,s=_extract([0,2,6,10,6,2,0],0,6,3,1,-1)
        self.assertEqual(s['near_crossings'],[2,4])

    def test_insufficient_support_preserves_morphology_sentinels(self):
        result=measure_spike_features([1,3,1],{'begin':0,'end':2,'sg_scan':1},definition())
        self.assertEqual((result['jag_count'],result['fit_score'],result['area_ratio']),(-1,-1,-1))
        self.assertEqual(result['morphology']['status'],'unavailable')

    def test_schema_three_requires_companion_and_excludes_restore(self):
        valid=definition()
        self.assertEqual(load_spike_definition(valid),valid)
        for field,value in (('measurement_source','prepared_signal'),('morphology_enabled',False),('treat_overlap_as_spike',False)):
            with self.assertRaises(ValueError):load_spike_definition(valid|{field:value})
        for changes in ({'jag_threshold':True},{'fit_score_threshold':float('nan')},{'unexpected':1}):
            invalid=deepcopy(valid);invalid['morphology'].update(changes)
            with self.assertRaises(ValueError):load_spike_definition(invalid)

    def test_batch_is_scalar_and_does_not_mutate_inputs(self):
        signal=[0,2,6,10,6,2,0];peaks=[{'begin':0,'end':6,'sg_scan':3}];d=definition()
        before=deepcopy((signal,peaks,d))
        measured=measure_spike_peaks(signal,peaks,d)
        self.assertEqual((signal,peaks,d),before)
        self.assertEqual(measured[0]['measurement_source'],'baseline_corrected_companion')
        self.assertNotIn('signal',measured[0]['morphology'])
        json.dumps(measured,allow_nan=False)

    def test_broad_jag_overlap_and_poor_fit_are_separate(self):
        features={'status':'evaluated_subset','exhalf_width_scan':8,'near_base_width_scan':12,
                  'area_ratio':.5,'jag_count':1,'fit_score':.8}
        result=classify_spike(features,height=200,offscaled=False,definition=definition())
        self.assertFalse(result['spike_detected']);self.assertTrue(result['overlap_spike'])
        self.assertTrue(result['current_spike']);self.assertEqual(result['peak_deformity'],'poor_fit')
        features['jag_count']=0
        result=classify_spike(features,height=200,offscaled=False,definition=definition())
        self.assertFalse(result['current_spike']);self.assertEqual(result['peak_deformity'],'poor_fit')

    def test_narrow_branches_and_outer_gate_skip_fit_deformity(self):
        for f,b,h in ((8,2,200),(2,12,200),(8,12,100)):
            features={'status':'evaluated_subset','exhalf_width_scan':f,'near_base_width_scan':b,
                      'area_ratio':.9,'jag_count':1,'fit_score':-1}
            result=classify_spike(features,height=h,offscaled=False,definition=definition())
            self.assertEqual(result['peak_deformity'],'none')

    def test_fit_threshold_is_strict_and_uncomputed_sentinel_is_not_hidden(self):
        features={'status':'evaluated_subset','exhalf_width_scan':-1,'near_base_width_scan':12,
                  'area_ratio':-1,'jag_count':-1,'fit_score':.9}
        self.assertEqual(classify_spike(features,height=200,offscaled=False,definition=definition())['peak_deformity'],'none')
        features['fit_score']=-1
        self.assertEqual(classify_spike(features,height=200,offscaled=False,definition=definition())['peak_deformity'],'poor_fit')

    def test_invalid_numeric_inputs_are_bounded(self):
        for values in ([float('nan')],[True],[2**31]):
            with self.assertRaises(ValueError):jag_metrics(values,.5)
        with self.assertRaises(ValueError):gaussian_fit_score([1,2,1],0,2,1)


if __name__=='__main__':unittest.main()
