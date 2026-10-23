from copy import deepcopy
import json
import unittest
from genemapper_open.spike import (measure_spike_features,measure_spike_peaks,classify_spike,load_spike_definition)
from genemapper_open.str_demo import make_spike_demo_definition


def features(f=8,b=12,r=.5,j=-1):
    return {'status':'evaluated_subset','exhalf_width_scan':f,'near_base_width_scan':b,'area_ratio':r,'jag_count':j}


class SpikeTests(unittest.TestCase):
    def measure(self,signal,**settings):
        definition=make_spike_demo_definition();definition.update(settings)
        return measure_spike_features(signal,{'begin':0,'end':len(signal)-1,'sg_scan':len(signal)//2},definition)

    def classify(self,observed=None,*,height=200,offscale=False,**settings):
        definition=make_spike_demo_definition();definition.update(settings)
        return classify_spike(features() if observed is None else observed,height=height,offscaled=offscale,definition=definition)

    def test_symmetric_widths_integer_area_and_ratio(self):
        measured=self.measure([0,2,6,10,6,2,0],near_base_fraction=.2)
        self.assertEqual(measured['exhalf_width_scan'],2.5)
        self.assertEqual(measured['near_base_width_scan'],4)
        self.assertEqual(measured['near_base_area'],26)
        self.assertAlmostEqual(measured['area_ratio'],25/26)
        self.assertEqual(measured['jag_count'],-1)

    def test_one_sided_truncation_mirrors_width_and_integer_area(self):
        measured=measure_spike_features([6,10,6,2,0],{'begin':0,'end':4,'sg_scan':1},
            make_spike_demo_definition()|{'near_base_fraction':.5})
        self.assertEqual(measured['exhalf_width_scan'],2.5)
        self.assertEqual(measured['half_truncation'],[True,False])
        self.assertEqual(measured['near_base_area'],26)

    def test_double_truncation_does_not_mirror(self):
        measured=self.measure([6,10,6],near_base_fraction=.5)
        self.assertEqual(measured['exhalf_width_scan'],2)
        self.assertEqual(measured['near_base_area'],22)
        self.assertEqual(measured['base_truncation'],[True,True])

    def test_max_search_keeps_original_tie_distance(self):
        signal=[10,0,0,0,10,2,0,10,0]
        measured=measure_spike_features(signal,{'begin':0,'end':8,'sg_scan':5},make_spike_demo_definition())
        self.assertEqual(measured['measurement_center'],7)

    def test_near_base_cap_changes_only_base_measurement(self):
        signal=[0,2,6,10,6,2,0]
        uncapped=self.measure(signal,near_base_fraction=.5)
        capped=self.measure(signal,near_base_fraction=.5,max_base_height=2)
        self.assertEqual(capped['exhalf_width_scan'],uncapped['exhalf_width_scan'])
        self.assertEqual(capped['near_base_width_scan'],4)
        self.assertEqual(uncapped['near_base_width_scan'],2.5)

    def test_zero_signal_retains_uncomputed_output_sentinels(self):
        measured=self.measure([0]*7)
        self.assertEqual([measured[k] for k in ('exhalf_width_scan','near_base_width_scan','near_base_area','raw_height','area_ratio')],[-1]*5)

    def test_base_shortcut_ignores_offscale_and_area(self):
        result=self.classify(features(b=9,r=-1),offscale=True)
        self.assertTrue(result['spike_detected']);self.assertTrue(result['current_spike'])
        self.assertEqual(result['threshold_used'],10)

    def test_height_and_base_outer_gates(self):
        self.assertFalse(self.classify(features(b=1),height=100)['current_spike'])
        self.assertFalse(self.classify(features(b=-1))['current_spike'])
        self.assertTrue(self.classify(features(b=1),height=100.001)['current_spike'])

    def test_width_and_area_equalities_are_strict(self):
        self.assertFalse(self.classify(features(f=6,b=10,r=1))['current_spike'])
        overlap=self.classify(features(f=5,b=10,r=.8))
        self.assertFalse(overlap['spike_detected']);self.assertTrue(overlap['overlap_spike'])
        detected=self.classify(features(f=5,b=10,r=.8001))
        self.assertTrue(detected['spike_detected']);self.assertEqual(detected['threshold_used'],6)

    def test_offscale_redirects_narrow_half_width_to_overlap(self):
        result=self.classify(features(f=5,b=10,r=1),offscale=True)
        self.assertFalse(result['spike_detected']);self.assertTrue(result['overlap_spike']);self.assertTrue(result['current_spike'])

    def test_overlap_and_current_spike_are_distinct(self):
        result=self.classify(features(f=5,b=10,r=0),treat_overlap_as_spike=False)
        self.assertTrue(result['overlap_spike']);self.assertFalse(result['current_spike'])
        self.assertEqual(result['threshold_used'],10)

    def test_jag_count_branch_does_not_treat_sentinel_as_jag(self):
        for j,expected in ((-1,False),(0,False),(1,True)):
            self.assertIs(self.classify(features(f=-1,b=10,j=j))['overlap_spike'],expected)

    def test_bp_units_need_separate_measurements(self):
        unknown=self.classify(units='bp')
        self.assertEqual(unknown['status'],'unavailable');self.assertIsNone(unknown['current_spike'])
        result=self.classify(features()|{'exhalf_width_bp':1,'near_base_width_bp':2},units='bp')
        self.assertTrue(result['current_spike'])
        json.dumps(unknown,allow_nan=False)

    def test_invalid_or_missing_measurements_are_not_assumed_false(self):
        self.assertEqual(self.classify({'status':'unavailable'})['status'],'unavailable')
        with self.assertRaises(ValueError):self.classify(features(r=float('nan')))
        with self.assertRaises(ValueError):self.classify(offscale=1)

    def test_definitions_reject_unknown_duplicate_advanced_or_unbounded_inputs(self):
        with self.assertRaises(ValueError):load_spike_definition('{"schema_version":1,"schema_version":1}')
        for key,value in (('morphology_enabled',True),('units','auto'),('near_base_fraction',0),('max_base_height',float('inf')),('min_height',True),('extra',1)):
            definition=make_spike_demo_definition();definition[key]=value
            with self.assertRaises(ValueError):load_spike_definition(definition)

    def test_batch_measurements_preserve_inputs_without_arrays_in_output(self):
        signal=[0,2,6,10,6,2,0];peaks=[{'begin':0,'end':6,'sg_scan':3}];definition=make_spike_demo_definition()
        before=deepcopy((signal,peaks,definition))
        result=measure_spike_peaks(signal,peaks,definition)
        self.assertEqual(result,[measure_spike_features(signal,peaks[0],definition)])
        self.assertEqual((signal,peaks,definition),before)
        self.assertNotIn('"signal":',json.dumps(result,allow_nan=False))
        for bad in ([True]*7,[-1]*7,[.5]*7):
            with self.assertRaises(ValueError):measure_spike_peaks(bad,peaks,definition)
