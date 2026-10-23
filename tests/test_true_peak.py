from copy import deepcopy
import json
import math
import unittest
from genemapper_open.true_peak import (_collector,_model_from_sides,_areas,_true_width,
    _measure_true_peak,two_point_gaussian,restore_peak)
from genemapper_open.peak_morphology import _extract
from genemapper_open.calibration import build_calibration
from genemapper_open.small_svd import SOLVER_ID
from genemapper_open.spike import load_spike_definition,classify_spike
from genemapper_open.str_demo import make_morphology_demo
from genemapper_open.rounding import round_scan


def definition():
    result=make_morphology_demo()['settings']['spike_definition']
    result.update(schema_version=4,treat_overlap_as_spike=False,
        true_peak={'polynomial_solver':SOLVER_ID,'max_denominator_scan2':10000})
    return result


class TruePeakTests(unittest.TestCase):
    def test_rounding_nearest_half_neighbors_uses_exact_binary_value(self):
        self.assertEqual(round_scan(math.nextafter(.5,0)),0)
        self.assertEqual(round_scan(.5),1)
        self.assertEqual(round_scan(math.nextafter(.5,1)),1)
        self.assertEqual(round_scan(-.5),0)
        self.assertEqual(round_scan(math.nextafter(-.5,-1)),-1)
        with self.assertRaises(ValueError):round_scan(float('nan'))
        with self.assertRaises(ValueError):round_scan(True)

    def test_true_width_gate_uses_java_round_not_rounded_addition(self):
        model={'height':4,'center':math.nextafter(.5,0),'denominator_scan2':2}
        self.assertEqual(_true_width([0,8,2,0],0,3,model,8,.5)['width_scan'],-1)

    def test_collector_p2_equality_and_stopping_after_four_accepted_points(self):
        signal=[0,12,10,6,3,2,1,.5,.25,9]
        three=_collector(signal,0,9,3,1,1)
        two=_collector(signal,0,9,3,1,2)
        self.assertEqual(three['intersection'],(5,2))
        self.assertEqual([p[0] for p in three['consecutive']],[5,6,7])
        self.assertEqual([p[0] for p in three['gaussian']],[5,6,7,8])
        self.assertEqual(three['maximum'],(5,2));self.assertEqual(three['last_scanned'],8)
        self.assertEqual(two['intersection'],(4,3))
        self.assertEqual(two['line_fit']['solver'],'two_point_line')

    def test_collector_max_includes_rejected_points_beyond_consecutive_list(self):
        signal=[0,0,10,6,3,2,8,2,1,.5,9]
        side=_collector(signal,0,10,3,1,2)
        self.assertEqual(side['intersection'],(4,3))
        self.assertEqual([p[0] for p in side['consecutive']],[4,5,6])
        self.assertEqual([p[0] for p in side['gaussian']],[4,5,8,9])
        self.assertEqual(side['maximum'],(6,8));self.assertEqual(side['last_scanned'],9)

    def test_nonpositive_stop_is_not_a_crossover(self):
        side=_collector([0,0,10,6,0,2,8,2,1,.5,9],0,10,3,1,2)
        self.assertEqual(side['status'],'no_crossover')
        self.assertIsNone(side['intersection']);self.assertEqual(side['gaussian'],[])

    def test_two_point_fit_analytic_formula_strict_denominator_and_height_gates(self):
        c=4/math.log(2)
        fit=two_point_gaussian((-2,4),(-4,.5),height_cap=8,denominator_limit=10)
        self.assertAlmostEqual(fit['height'],8)
        self.assertAlmostEqual(fit['denominator_scan2'],c)
        self.assertIsNone(two_point_gaussian((-2,4),(2,4),height_cap=16,denominator_limit=10))
        self.assertIsNone(two_point_gaussian((-2,4),(-4,.5),height_cap=7.99,denominator_limit=10))
        exact=two_point_gaussian((0,1),(1,math.exp(-1)),height_cap=1,denominator_limit=2)
        self.assertEqual(exact,{'height':1,'denominator_scan2':1})
        self.assertIsNone(two_point_gaussian((0,1),(1,math.exp(-1)),height_cap=1,denominator_limit=1))
        self.assertIsNone(two_point_gaussian((0,0),(1,1),height_cap=1,denominator_limit=10))

    def test_full_analytic_fixture_pair_order_width_and_literal_area(self):
        g=lambda x:8*2**(-x*x/4)
        signal=[g(i-5) for i in range(11)];signal[5]=16
        whole,_,_,_,selection=_extract(signal,0,10,5,1/8,-1)
        self.assertEqual(selection['near_crossings'],[2,8])
        measured=_measure_true_peak(signal,0,10,selection,whole,
            {'polynomial_solver':SOLVER_ID,'max_denominator_scan2':10000,'near_base_fraction':1/8})
        self.assertEqual(measured['status'],'evaluated')
        self.assertAlmostEqual(measured['height'],8)
        self.assertAlmostEqual(measured['center'],5)
        self.assertAlmostEqual(measured['model']['denominator_scan2'],4/math.log(2))
        self.assertEqual(measured['fitting']['merged_indices'],[3,7,2,8,1,9,0,10])
        self.assertEqual([p['indices'] for p in measured['fitting']['accepted_pairs']],[[3,2],[3,8],[3,1],[3,9]])
        self.assertEqual(measured['fitting']['attempted_pairs'],5)
        self.assertAlmostEqual(measured['width_scan'],2*(3+(g(3)-1)/(g(3)-.5))+1)
        expected=sum(g(x) for x in range(-3,4))+g(5)+2*g(4)+g(3)
        self.assertAlmostEqual(measured['area'],expected)
        self.assertAlmostEqual(measured['excess_area_ratio'],16/(16+2*(g(1)+4+g(3))))
        self.assertEqual(measured,json.loads(json.dumps(measured,allow_nan=False)))

    def test_true_area_keeps_duplicate_right_boundary_and_strict_interior(self):
        model={'height':8,'center':3,'denominator_scan2':1/math.log(2)}
        selection={'origin':1,'relative_center':2,'truncated':[False,False],'height':16,'center':3}
        area=_areas([1,2,6,16,6,2,1],0,6,selection,[2,6,16,6,2],model,(2,6),(4,6))
        self.assertEqual(area['area'],27)
        self.assertEqual(area['excess_area_ratio'],.25)
        self.assertEqual((area['left_tail_added'],area['right_tail_added']),(1,2))

    def test_one_sided_mirrors_keep_literal_tail_asymmetry(self):
        for signal,origin,center,truncated,left,right,expected in (
            ([6,16,6,2,1],-1,1,[True,False],None,(2,6),28),
            ([1,2,6,16,6],1,3,[False,True],(2,6),None,26)):
            selection={'origin':origin,'relative_center':2,'truncated':truncated,'height':16,'center':center}
            model={'height':8,'center':center,'denominator_scan2':1/math.log(2)}
            area=_areas(signal,0,4,selection,[2,6,16,6,2],model,left,right)
            self.assertEqual(area['relative_intersections'],[1,3])
            self.assertEqual(area['area'],expected)

    def test_width_requires_strictly_smaller_true_height(self):
        model={'height':8,'center':3,'denominator_scan2':2}
        width=_true_width([0,1,4,8,4,1,0],0,6,model,8,.125)
        self.assertEqual(width['width_scan'],-1)
        model['height']=4
        width=_true_width([0,1,4,8,4,1,0],0,6,model,8,.25)
        self.assertEqual(width['width_scan'],5)

    def test_empty_sides_keep_midpoint_and_no_invented_model(self):
        side={'maximum':None,'consecutive':[],'gaussian':[],'intersection':None}
        model,history=_model_from_sides(side,side,0,9,10,10000)
        self.assertIsNone(model);self.assertEqual(history['selected_center'],4)

    def test_true_center_tie_prefers_right_measured_candidate(self):
        left={'maximum':(2,3),'consecutive':[],'gaussian':[],'intersection':(2,3)}
        right={'maximum':(8,3),'consecutive':[],'gaussian':[],'intersection':(8,3)}
        model,history=_model_from_sides(left,right,0,10,10,10000)
        self.assertEqual(history['selected_center'],8)

    def test_missing_relative_intersection_keeps_area_sentinel(self):
        selection={'origin':0,'relative_center':2,'truncated':[False,False],'height':8,'center':2}
        area=_areas([1,2,8,2,1],0,4,selection,[1,2,8,2,1],
            {'height':4,'center':2,'denominator_scan2':2},None,(3,2))
        self.assertEqual((area['area'],area['excess_area_ratio']),(-1,-1))

    def test_schema4_explicit_solver_and_schema3_boundary_preserved(self):
        d=definition();self.assertEqual(load_spike_definition(d),d)
        invalid=deepcopy(d);invalid['true_peak']['polynomial_solver']='generic'
        with self.assertRaises(ValueError):load_spike_definition(invalid)
        invalid=deepcopy(d);invalid['true_peak']['max_denominator_scan2']=float('inf')
        with self.assertRaises(ValueError):load_spike_definition(invalid)
        invalid=deepcopy(d);invalid.update(schema_version=3);invalid.pop('true_peak')
        with self.assertRaises(ValueError):load_spike_definition(invalid)

    def peak(self,*,height=8,center=5.9,area=27):
        return {'height':16,'scan':5,'size_bp':110,'area':34,'begin':1,'end':9,'fwhm_scan':2,
            'spike_features':{'morphology':{'true_peak':{'height':height,'center':center,'area':area}}}}

    def classification(self):
        return {'status':'evaluated_subset','spike_detected':False,'current_spike':False,
            'overlap_spike':True,'true_peak_restoration_requested':True}

    def test_restore_truncates_center_uses_integer_endpoint_map_and_preserves_width(self):
        curve=build_calibration([(0,100),(5,110),(10,140)],'lsq2')
        peak=self.peak()|{'end':8};before=deepcopy(peak)
        restored=restore_peak(peak,self.classification(),calibration=curve,signal_length=11)
        self.assertEqual(peak,before)
        self.assertEqual(restored['scan'],5) # trunc5.9, not round6
        self.assertAlmostEqual(restored['area_bp'],(curve.standard_map_size(8)-curve.standard_map_size(1))/7*27)
        self.assertNotAlmostEqual(restored['area_bp'],4*27) # not derivative at integer center5
        self.assertEqual(restored['fwhm_scan'],2)
        self.assertEqual(restored['true_peak_restoration']['writes'],['height','scan','size_bp','area','area_bp'])

    def test_restore_height_precedes_missing_map_and_geometry_guards(self):
        curve=build_calibration([(0,100),(10,120)],'linear')
        for peak,calibration in ((self.peak(),None),(self.peak(center=99),curve),
            (self.peak()|{'begin':3,'end':3},curve),(self.peak()|{'begin':-1},curve)):
            restored=restore_peak(peak,self.classification(),calibration=calibration,signal_length=11)
            self.assertEqual(restored['height'],8)
            self.assertEqual(restored['scan'],5)
            self.assertEqual(restored['area'],34)
            self.assertEqual(restored['true_peak_restoration']['status'],'height_only')

    def test_negative_true_area_is_preserved_without_clamping(self):
        restored=restore_peak(self.peak(area=-1),self.classification(),
            calibration=build_calibration([(0,100),(10,120)],'linear'),signal_length=11)
        self.assertEqual(restored['area'],-1);self.assertEqual(restored['area_bp'],-2)
        self.assertFalse(restored['true_peak_restoration']['area_available'])

    def test_absent_true_height_and_detected_branch_do_not_write(self):
        for peak,c in ((self.peak(height=-1),self.classification()),
            (self.peak(),self.classification()|{'spike_detected':True})):
            result=restore_peak(peak,c,calibration=None,signal_length=11)
            self.assertEqual(result['true_peak_restoration']['writes'],[])
            self.assertEqual(result['height'],16)

    def test_fresh_contract_rejects_second_application(self):
        peak=restore_peak(self.peak(),self.classification(),calibration=None,signal_length=11)
        with self.assertRaises(ValueError):restore_peak(peak,self.classification(),calibration=None,signal_length=11)

    def test_classifier_records_original_height_and_requests_restore_only_for_overlap(self):
        f={'status':'evaluated_subset','near_base_width_scan':12,'exhalf_width_scan':8,'area_ratio':.8,
            'jag_count':1,'fit_score':.8}
        c=classify_spike(f,height=200,offscaled=False,definition=definition())
        self.assertTrue(c['true_peak_restoration_requested']);self.assertFalse(c['current_spike'])
        c=classify_spike(f|{'near_base_width_scan':1},height=200,offscaled=False,definition=definition())
        self.assertFalse(c['true_peak_restoration_requested']);self.assertTrue(c['spike_detected'])


if __name__=='__main__':unittest.main()
