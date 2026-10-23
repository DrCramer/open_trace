from copy import deepcopy
import json
import unittest
from genemapper_open.calibration import build_calibration
from genemapper_open.post_sizing_quality import load_sizing_quality_definition,standard_dimensions,evaluate_post_sizing_quality
from genemapper_open.server import analyze
from genemapper_open.str_demo import make_str_demo,make_quality_demo_definition,make_ladder_quality_demo_definition
from genemapper_open.str_workflow import run_str_workflow,compact_analysis
from genemapper_open.marker_quality_workflow import evaluate_marker_quality


def definition(**changes):
    return {'schema_version':1,'context':'fresh_hid_no_override','width_conversion':'unrounded_standard_map_span',
        'warning_width_bp':2,'broad_weight':.5,'thresholds':{'good':.75,'bad':.25},**changes}


def observed(scan=200,width=8,begin=190,end=210,bp=20):
    return {'scan':scan,'begin':begin,'end':end,'fwhm_scan':width,'matched_bp':bp}


class StandardMapTests(unittest.TestCase):
    def test_local_southern_exterior_uses_endpoint_pair_without_public_extrapolation(self):
        curve=build_calibration([[100,50],[200,90],[350,135],[600,180]],'local_rational')
        self.assertAlmostEqual(curve.standard_map_size(90),46)
        self.assertAlmostEqual(curve.standard_map_size(650),189)
        self.assertIsNone(curve.size(90));self.assertIsNone(curve.size(650))
        self.assertFalse(curve.diagnostics['extrapolation'])

    def test_crossing_anchor_span_uses_interior_and_exterior_map_values(self):
        curve=build_calibration([[100,50],[200,90],[350,135],[600,180]],'local_rational')
        result=standard_dimensions(observed(scan=100,begin=90,end=110,width=8),curve)
        expected=8*(curve.size(110)-46)/20
        self.assertAlmostEqual(result['fwhm_bp'],expected)
        self.assertTrue(result['outside_anchor_bounds'])
        self.assertNotAlmostEqual(result['fwhm_bp'],8*.4)

    def test_spline_exterior_is_endpoint_secant_not_endpoint_derivative(self):
        curve=build_calibration([[100,10],[200,40],[300,90]],'natural_cubic')
        self.assertAlmostEqual(curve.standard_map_size(400),140)
        self.assertEqual(curve.standard_map_size(50),-1)

    def test_polynomial_exterior_evaluates_fit_instead_of_secant(self):
        for model,degree in (('lsq2',2),('lsq3',3)):
            anchors=[[x,(x/100)**degree] for x in (100,200,300,400)]
            curve=build_calibration(anchors,model)
            self.assertAlmostEqual(curve.standard_map_size(50),.5**degree,places=8)
            self.assertAlmostEqual(curve.standard_map_size(500),5**degree,places=8)
            self.assertIsNone(curve.size(50))

    def test_standard_morphology_does_not_round_endpoint_bp(self):
        curve=build_calibration([[100,10.1234567],[200,20.2345678]],'linear')
        measured=standard_dimensions(observed(begin=101,end=102,width=.5),curve)
        left,right=curve.size(101),curve.size(102)
        self.assertEqual(measured['fwhm_bp'],.5*(right-left))
        self.assertNotEqual(measured['fwhm_bp'],.5*(round(right,5)-round(left,5)))
        self.assertFalse(measured['position_rounding_applied'])

    def test_integer_bounds_are_cast_before_lookup_and_denominator(self):
        curve=build_calibration([[100,10],[200,20],[300,30]],'linear')
        measured=standard_dimensions(observed(begin=190.9,end=210.1),curve)
        self.assertEqual([measured['begin_scan'],measured['end_scan']],[190,210])
        self.assertAlmostEqual(measured['fwhm_bp'],.8)

    def test_negative_or_zero_integer_span_is_unavailable(self):
        curve=build_calibration([[100,10],[200,40],[300,90]],'local_rational')
        self.assertEqual(standard_dimensions(observed(begin=20,end=110),curve)['reason'],
                         'negative_or_sentinel_standard_map_endpoint')
        self.assertEqual(standard_dimensions(observed(begin=190.1,end=190.9),curve)['reason'],
                         'nonpositive_integer_peak_span')


class PostSizingQualityTests(unittest.TestCase):
    def evaluate(self,peaks,**changes):
        curve=build_calibration([[100,10],[200,20],[300,30]],'linear')
        return evaluate_post_sizing_quality({'status':'evaluated','score':.8},peaks,curve,
            definition(**changes),reference_channel='DATA105')

    def test_multiple_broad_matched_peaks_penalize_once(self):
        result=self.evaluate([observed(width=30),observed(scan=250,width=40,begin=240,end=260)])
        self.assertEqual(result['score'],.4);self.assertEqual(result['broad_metric'],1)
        self.assertEqual(result['penalty']['trigger_index'],0)
        self.assertEqual(result['category']['code'],1)
        self.assertFalse(result['calibration_rejection_requested'])

    def test_strict_width_comparison_and_zero_weight(self):
        result=self.evaluate([observed(width=20)])
        self.assertEqual(result['score'],.8);self.assertEqual(result['broad_metric'],0)
        result=self.evaluate([observed(width=20.01)],broad_weight=0)
        self.assertEqual(result['score'],.8);self.assertEqual(result['broad_metric'],1)

    def test_unmatched_broad_peak_does_not_affect_sq(self):
        result=self.evaluate([observed(width=10),observed(width=100,bp=None)])
        self.assertEqual(len(result['measurements']),1)
        self.assertEqual(result['score'],.8)

    def test_crossing_anchor_boundary_is_measured(self):
        result=self.evaluate([observed(scan=100,begin=90,end=110,width=20)])
        self.assertEqual(result['status'],'evaluated')
        self.assertTrue(result['measurements'][0]['outside_anchor_bounds'])

    def test_missing_base_calibration_matched_or_valid_bounds_is_not_a_pass(self):
        curve=build_calibration([[100,10],[200,20],[300,30]],'linear')
        for base,peaks,calibration in ((None,[observed()],curve),({'status':'evaluated','score':.8},[observed()],None),
            ({'status':'evaluated','score':.8},[observed(bp=None)],curve),
            ({'status':'evaluated','score':.8},[observed(begin=190,end=190)],curve)):
            result=evaluate_post_sizing_quality(base,peaks,calibration,definition(),reference_channel='DATA105')
            self.assertEqual(result['status'],'unavailable');self.assertIsNone(result['score'])
            self.assertIsNone(result['category']);json.dumps(result,allow_nan=False)

    def test_post_penalty_bad_equality_is_check_and_lower_is_fail(self):
        exact=self.evaluate([observed(width=30)],broad_weight=.6875)
        self.assertEqual(exact['score'],.25);self.assertEqual(exact['category']['code'],1)
        below=self.evaluate([observed(width=30)],broad_weight=.7)
        self.assertEqual(below['category']['code'],2)

    def test_strict_complete_definition_and_duplicate_validation(self):
        self.assertEqual(load_sizing_quality_definition(json.dumps(definition())),definition())
        for key,value in (('schema_version',True),('context','override'),('width_conversion','local_slope'),
                          ('warning_width_bp',float('nan')),('broad_weight',1.1),('extra',0)):
            invalid=definition();invalid[key]=value
            with self.assertRaises(ValueError):load_sizing_quality_definition(invalid)
        with self.assertRaises(ValueError):load_sizing_quality_definition('{"schema_version":1,"schema_version":1}')
        with self.assertRaises(ValueError):load_sizing_quality_definition(definition(thresholds={'good':.1,'bad':.9}))


class PostSizingWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture=make_str_demo();x=cls.fixture
        cls.settings=x['settings']|{'sizing_quality_definition':definition(warning_width_bp=.1,broad_weight=.8)}
        cls.sample=analyze(x['sample_bytes'],'sample.fsa',**cls.settings)
        cls.ladder=analyze(x['ladder_bytes'],'ladder.fsa',**cls.settings)
        cls.report=run_str_workflow(cls.sample,cls.ladder,x['kit'])

    def test_all_matched_peaks_including_first_last_have_bp_widths(self):
        post=self.sample['sizing']['post_quality']
        self.assertEqual(post['status'],'evaluated');self.assertEqual(len(post['measurements']),6)
        self.assertTrue(post['measurements'][0]['outside_anchor_bounds'])
        self.assertTrue(post['measurements'][-1]['outside_anchor_bounds'])
        self.assertEqual(post['category']['code'],2)

    def test_low_post_score_does_not_erase_calibration_or_recall_sample(self):
        x=self.fixture;plain=analyze(x['sample_bytes'],'sample.fsa',**x['settings'])
        self.assertEqual(self.sample['sizing']['anchors'],plain['sizing']['anchors'])
        self.assertEqual(self.sample['channels'],plain['channels'])
        self.assertEqual(self.sample['sizing']['status'],'accepted')
        self.assertTrue(self.sample['sizing']['calibration_applied'])
        self.assertEqual([c['label'] for c in self.report['calls']],['B','C'])

    def test_post_score_flows_to_confidence_and_ladder_sq_gate(self):
        recipe=make_quality_demo_definition();recipe['sample_sq']={'source':'post_sizing_quality'}
        quality=evaluate_marker_quality(self.report,recipe)
        self.assertEqual(quality['sample_sq'],self.sample['sizing']['post_quality']['score'])
        self.assertLess(quality['markers'][0]['call_scores'][0]['after_sample_sq'],.25)
        ladder_recipe=make_ladder_quality_demo_definition();ladder_recipe['ladder_sq']={'source':'post_sizing_quality'}
        gated=run_str_workflow(self.sample,self.ladder,self.fixture['kit'],ladder_quality_definition=ladder_recipe)
        self.assertFalse(gated['quality']['ladder_gq']['gate_passed'])
        self.assertEqual(gated['quality']['ladder_gq']['sq_category']['code'],2)
        self.assertEqual(gated['calls'],[])

    def test_requested_post_source_never_falls_back_to_base(self):
        recipe=make_quality_demo_definition();recipe['sample_sq']={'source':'post_sizing_quality'}
        report=deepcopy(self.report);report['sizing'].pop('post_quality')
        result=evaluate_marker_quality(report,recipe)
        self.assertIsNone(result['sample_sq']);self.assertEqual(result['status'],'unavailable')

    def test_compact_reports_and_settings_retain_exact_quality_context(self):
        compact=run_str_workflow(compact_analysis(self.sample),compact_analysis(self.ladder),self.fixture['kit'])
        self.assertEqual(compact,self.report)
        changed=deepcopy(self.sample);changed['peak_settings']['sizing_quality_definition']['broad_weight']=.5
        self.assertEqual(run_str_workflow(changed,self.ladder,self.fixture['kit'])['status'],'rejected')

    def test_unsupported_analysis_context_is_rejected(self):
        with self.assertRaises(ValueError):analyze(self.fixture['sample_bytes'],'x.fsa',sizing_quality_definition=definition())
        with self.assertRaises(ValueError):analyze(self.fixture['sample_bytes'],'x.fsa',
            analysis_engine='morphology_ratio',sizing_quality_definition=definition())
