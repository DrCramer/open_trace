from copy import deepcopy
import json
import unittest
from genemapper_open.ladder_quality import score_ladder_marker,ladder_spacing_tolerance


def recipe():
    return {'min_height_ratio':.2,'spike_metric':0,'spike_weight':.5,'offscale_metric':0,'offscale_weight':.5,
        'spacing':{'enabled':True,'max_distance_bp':12,'curve':[{'distance_bp':0,'fraction':.2,'interpolate':False}],
                   'exceptions':[],'excluded_alleles':[]}}


def call(name,size,height=100,*,peak_bp=None,bin_name=None,center=None,ol=False):
    return {'name':name,'size_bp':size,'peak_bp':size if peak_bp is None else peak_bp,'height':height,
        'off_ladder':ol,'bin':None if ol else {'name':bin_name or name,'center_bp':size if center is None else center}}


class LadderQualityTests(unittest.TestCase):
    def score(self,calls,expected=('A','B'),settings=None):
        return score_ladder_marker(calls,list(expected),recipe() if settings is None else settings)

    def test_expected_membership_is_case_sensitive_on_bin_names_not_call_names_or_count(self):
        r=recipe();r['spacing']['enabled']=False
        calls=[call('display',100,bin_name='A'),call('B',104),call('OL',108,ol=True)]
        self.assertEqual(self.score(calls,('A','A','B'),r)['gq'],1)
        missing=self.score(calls,('a','B'),r)
        self.assertEqual(missing['missing_expected_bins'],['a'])
        self.assertEqual(missing['gq'],0)

    def test_empty_calls_fail_even_without_expected_bins(self):
        result=self.score([],())
        self.assertEqual(result['gq'],0);self.assertEqual(result['allele_number_metric'],-2)
        self.assertEqual(result['events'][0]['criterion'],'no_called_alleles')

    def test_first_two_sort_by_allele_size_not_height_or_peak_bp(self):
        r=recipe();r['spacing']['enabled']=False
        result=self.score([call('B',104,200,peak_bp=90),call('A',100,100,peak_bp=120)],settings=r)
        self.assertEqual(result['sorted_call_names'],['A','B'])
        self.assertEqual(result['first_two_height_ratio'],.5)
        self.assertEqual(result['gq'],0)

    def test_first_two_half_equality_fails_and_above_passes(self):
        for first,expected in ((50,0),(50.001,1),(100,1)):
            result=self.score([call('A',100,first),call('B',104)])
            self.assertEqual(result['gq'],expected)

    def test_min_max_includes_extra_ol_and_equality_fails(self):
        r=recipe();r['spacing']['enabled']=False
        for height,gq in ((20,0),(20.001,1)):
            result=self.score([call('A',100),call('B',104),call('OL',108,height,ol=True)],settings=r)
            self.assertEqual(result['gq'],gq)
            self.assertEqual(result['first_two_height_ratio'],1)

    def test_ladder_artifact_weights_are_binary_gates_not_factors(self):
        for key in ('spike','offscale'):
            for state in (-2,-1,0,1):
                for weight in (0,.01,1):
                    r=recipe();r[key+'_metric']=state;r[key+'_weight']=weight
                    self.assertEqual(self.score([call('A',100),call('B',104)],settings=r)['gq'],
                                     0 if state==1 and weight>0 else 1)

    def test_tolerance_uses_integer_distance_and_special_zero_slot(self):
        for distance,expected in ((0,.2),(.9,.2),(4,.8),(4.9,.8),(5,1),(12,2.4)):
            self.assertAlmostEqual(ladder_spacing_tolerance(distance,'A','B',recipe())['tolerance_bp'],expected)
        self.assertIsNone(ladder_spacing_tolerance(12.01,'A','B',recipe())['tolerance_bp'])

    def test_linear_and_left_step_curve_rules(self):
        r=recipe();r['spacing']['curve']=[{'distance_bp':0,'fraction':.1,'interpolate':True},
                                        {'distance_bp':10,'fraction':.3,'interpolate':False}]
        self.assertAlmostEqual(ladder_spacing_tolerance(4.9,'A','B',r)['tolerance_bp'],.72)
        r['spacing']['curve'][0]['interpolate']=False
        self.assertAlmostEqual(ladder_spacing_tolerance(4.9,'A','B',r)['tolerance_bp'],.4)

    def test_directed_exception_priority_reverse_lookup_and_distance_gate(self):
        r=recipe();r['spacing']['exceptions']=[{'first':' B ','second':' A ','tolerance_bp':.1}]
        self.assertEqual(ladder_spacing_tolerance(4,'A','B',r)['tolerance_bp'],.1)
        r['spacing']['exceptions'].append({'first':'A','second':'B','tolerance_bp':.3})
        self.assertEqual(ladder_spacing_tolerance(4,'A','B',r)['tolerance_bp'],.3)
        self.assertIsNone(ladder_spacing_tolerance(13,'A','B',r)['tolerance_bp'])
        self.assertEqual(ladder_spacing_tolerance(4,'a','b',r)['source'],'integer_distance_curve')

    def test_negative_exception_falls_back_negative_curve_disables(self):
        r=recipe();r['spacing']['exceptions']=[{'first':'A','second':'B','tolerance_bp':-1}]
        self.assertEqual(ladder_spacing_tolerance(4,'A','B',r)['tolerance_bp'],.8)
        r['spacing']['curve'][0]['fraction']=-1
        self.assertIsNone(ladder_spacing_tolerance(4,'A','B',r)['tolerance_bp'])

    def test_spacing_strict_greater_boundary(self):
        r=recipe();r['spacing']['curve'][0]['fraction']=.5
        for observed,gq in ((106,1),(106.0001,0)):
            calls=[call('A',100),call('B',104,peak_bp=observed)]
            result=self.score(calls,settings=r)
            self.assertEqual(result['gq'],gq)
            self.assertEqual(result['spacing_checks'][0]['tolerance_bp'],2)

    def test_ol_between_calls_is_skipped_but_trailing_ol_leaves_missing_bin(self):
        middle=self.score([call('A',100),call('OL',102,ol=True),call('B',104)])
        self.assertEqual(middle['gq'],1)
        self.assertEqual(middle['spacing_checks'][0]['second'],'B')
        tail=self.score([call('A',100),call('B',104),call('OL',106,ol=True)])
        self.assertEqual(tail['gq'],0)
        self.assertEqual(tail['events'][-1]['criterion'],'pair_bin_missing')

    def test_exclusion_checks_start_only_and_is_case_insensitive(self):
        r=recipe();r['spacing']['exceptions']=[{'first':'A','second':'B','tolerance_bp':0}]
        calls=[call('A',100),call('B',104,peak_bp=105)]
        r['spacing']['excluded_alleles']=['b']
        self.assertEqual(self.score(calls,settings=r)['gq'],0)
        r['spacing']['excluded_alleles']=['a']
        self.assertEqual(self.score(calls,settings=r)['gq'],1)

    def test_empty_curve_disables_spacing_even_with_exception(self):
        r=recipe();r['spacing'].update(curve=[],exceptions=[{'first':'A','second':'B','tolerance_bp':0}])
        result=self.score([call('A',100),call('B',104,peak_bp=105)],settings=r)
        self.assertFalse(result['spacing_enabled']);self.assertEqual(result['gq'],1)

    def test_inputs_are_immutable_and_disabled_tolerance_is_strict_json(self):
        calls=[call('A',100),call('B',120)];r=recipe();original=deepcopy((calls,r))
        result=self.score(calls,settings=r)
        self.assertEqual((calls,r),original)
        self.assertEqual(result['spacing_checks'][0]['status'],'disabled')
        json.dumps(result,allow_nan=False)

    def test_invalid_settings_and_missing_or_zero_peaks_rejected(self):
        for key,value in (('min_height_ratio',True),('spike_metric',.5),('offscale_weight',float('nan'))):
            r=recipe();r[key]=value
            with self.assertRaises(ValueError):self.score([],settings=r)
        for value in (0,1e-300,None,float('nan')):
            bad=call('A',100);bad['height']=value
            with self.assertRaises(ValueError):self.score([bad],('A',))
        r=recipe();r['spacing']['curve']*=2
        with self.assertRaises(ValueError):self.score([],settings=r)
