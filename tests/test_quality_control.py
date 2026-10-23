from copy import deepcopy
import unittest
from genemapper_open.calibration import build_calibration
from genemapper_open.quality_control import (quality_category,profile_pullup_category,
    composite_control_category,positive_control,negative_control,zero_peak_metric,
    dimensions_in_bp,broad_standard_penalty)


class QualityControlTests(unittest.TestCase):
    def test_numeric_quality_strict_bad_inclusive_good(self):
        self.assertEqual([quality_category(s,good=.75,bad=.25)['code'] for s in (0,.249,.25,.749,.75,1)],
                         [2,2,1,1,0,0])
        with self.assertRaises(ValueError):quality_category(.5,good=.25,bad=.75)
        with self.assertRaises(ValueError):quality_category(float('nan'),good=.75,bad=.25)

    def test_profile_pullup_denominator_and_strict_threshold(self):
        result=profile_pullup_category([1,None,-2,0],threshold=.25)
        self.assertEqual((result['code'],result['denominator'],result['fraction']),(1,4,.25))
        self.assertEqual(profile_pullup_category([1,None,-2,0],threshold=.249)['code'],2)
        self.assertEqual(profile_pullup_category([None,-2],threshold=.5)['code'],-2)
        self.assertEqual(profile_pullup_category([],threshold=.5)['code'],-2)

    def test_composite_control_exact_codes_only(self):
        self.assertEqual([composite_control_category(x)['code'] for x in
            ([],[-2,-1],[.5,2],[0,-2],[0,1])],[-2,-2,-2,0,2])

    def test_positive_membership_is_not_multiset_equality(self):
        self.assertEqual(positive_control(['10','10'],['10','11'],is_y_marker=False)['cc'],0)
        self.assertEqual(positive_control(['10','11'],['10','10'],is_y_marker=False)['cc'],1)
        self.assertEqual(positive_control(['x','y'],['Y','X'],is_y_marker=False)['cc'],0)
        self.assertEqual(positive_control(['10'],['10 '],is_y_marker=False)['cc'],1)

    def test_missing_expected_is_skipped_and_no_call_gq_distinct(self):
        report=positive_control(['10'],None,is_y_marker=False)
        self.assertFalse(report['comparison_performed']);self.assertEqual(report['cc'],0)
        self.assertFalse(report['gq_forced_zero'])
        report=positive_control([],[],is_y_marker=False)
        self.assertEqual(report['cc'],0);self.assertTrue(report['gq_forced_zero'])
        self.assertFalse(positive_control([],None,is_y_marker=True)['gq_forced_zero'])
        with self.assertRaises(ValueError):positive_control(['Straße'],['STRASSE'],is_y_marker=False)

    def test_sensor_rules_are_case_sensitive_and_preserve_input(self):
        called,expected=['Y','X'],['X','Y'];before=deepcopy((called,expected))
        self.assertEqual(zero_peak_metric(2,called,quality_sensor=True,expected=expected)['value'],1)
        self.assertEqual((called,expected),before)
        self.assertEqual(zero_peak_metric(2,['x','Y'],quality_sensor=True,expected=expected)['value'],0)
        self.assertEqual(zero_peak_metric(0,[],quality_sensor=False,expected=None)['value'],1)
        self.assertEqual(zero_peak_metric(0,[],quality_sensor=True,expected=['X'])['value'],0)

    def test_negative_control_tests_metric_exact_zero_not_call_count(self):
        self.assertEqual([negative_control(value)['cc'] for value in (-2,-1,0,.5,1)],[0,0,1,0,0])

    def test_endpoint_span_scaling_differs_from_derivative_for_nonlinear_curve(self):
        curve=build_calibration([(0,10),(100,30),(200,90)],'linear')
        peak={'begin_scan':50,'end_scan':150,'fwhm_scan':10,'area':200}
        dimensions=dimensions_in_bp(peak,curve)
        self.assertAlmostEqual(dimensions['bp_per_scan_span'],.4)
        self.assertAlmostEqual(dimensions['fwhm_bp'],4)
        self.assertAlmostEqual(dimensions['area_bp'],80)
        peak['begin_scan']=-1
        with self.assertRaises(ValueError):dimensions_in_bp(peak,curve)
        peak.update(begin_scan=150,end_scan=250)
        self.assertEqual(dimensions_in_bp(peak,curve)['status'],'unavailable')

    def penalty(self,peaks,**kwargs):
        return broad_standard_penalty(.8,peaks,reference_channel='DATA105',warning_width_bp=1,
            weight=kwargs.pop('weight',.5),is_hid=kwargs.pop('is_hid',True),
            sq_overridden=kwargs.pop('sq_overridden',False),**kwargs)

    def test_broad_penalty_once_strict_width_and_matched_reference_only(self):
        peak={'channel':'DATA105','size_matched':True,'fwhm_bp':1}
        self.assertEqual(self.penalty([peak])['score'],.8)
        peaks=[dict(peak,channel='DATA1',fwhm_bp=10),dict(peak,size_matched=False,fwhm_bp=10)]
        self.assertFalse(self.penalty(peaks)['applied'])
        peaks += [dict(peak,fwhm_bp=2),dict(peak,fwhm_bp=3)]
        result=self.penalty(peaks)
        self.assertEqual(result['score'],.4);self.assertEqual(result['trigger_index'],2)
        self.assertFalse(result['calibration_rejection_requested'])

    def test_override_does_not_reset_metric_and_weight_zero_can_still_flag(self):
        peaks=[{'channel':'DATA105','size_matched':True,'fwhm_bp':2}]
        self.assertIsNone(self.penalty(peaks,sq_overridden=True)['broad_metric'])
        self.assertIsNone(self.penalty(peaks,is_hid=False)['broad_metric'])
        result=self.penalty(peaks,weight=0)
        self.assertEqual((result['score'],result['broad_metric']),(.8,1))
        with self.assertRaises(ValueError):self.penalty([{'channel':'DATA105','size_matched':True}])


if __name__=='__main__':unittest.main()
