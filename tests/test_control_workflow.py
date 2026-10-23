from copy import deepcopy
import unittest
from genemapper_open.str_demo import make_str_demo
from genemapper_open.str_workflow import analyze_str_files
from genemapper_open.control_workflow import evaluate_controls,attach_control_result
from genemapper_open.ladder_aggregation import aggregate_ladder_offsets


class ControlWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        fixture=make_str_demo();cls.kit=fixture['kit']
        cls.report=analyze_str_files(fixture['sample_bytes'],fixture['ladder_bytes'],fixture['kit'],settings=fixture['settings'])

    def definition(self,kind='Positive Control',expected=None,sensor=False):
        return {'schema_version':1,'sample_type':kind,'markers':{'ExampleM':{
            'expected_alleles':['B','C'] if expected is None else expected,
            'quality_sensor':sensor,'is_y_marker':False}}}

    def test_positive_match_mismatch_and_skipped_profile_have_distinct_states(self):
        report=evaluate_controls(self.report,self.definition())
        self.assertEqual(report['composite_control']['code'],0)
        self.assertTrue(report['all_control_comparisons_performed'])
        report=evaluate_controls(self.report,self.definition(expected=['A','B']))
        self.assertEqual(report['composite_control']['code'],2)
        self.assertTrue(report['markers'][0]['gq_forced_zero'])
        report=evaluate_controls(self.report,self.definition(expected=[]))
        self.assertEqual(report['composite_control']['code'],0)
        self.assertFalse(report['all_control_comparisons_performed'])

    def test_negative_sensor_exception_is_explicit_not_inferred_from_name(self):
        ordinary=evaluate_controls(self.report,self.definition('Negative Control'))
        sensor=evaluate_controls(self.report,self.definition('Negative Control',sensor=True))
        self.assertEqual(ordinary['composite_control']['code'],2)
        self.assertEqual(sensor['composite_control']['code'],0)

    def test_empty_negative_and_missing_sensor_have_opposite_zero_peak_states(self):
        empty=deepcopy(self.report);m=empty['allele_calling']['markers'][0];m['calls']=[];m['peak_review']=[]
        ordinary=evaluate_controls(empty,self.definition('Negative Control',expected=[]))
        sensor=evaluate_controls(empty,self.definition('Negative Control',sensor=True))
        self.assertEqual(ordinary['composite_control']['code'],0)
        self.assertEqual(sensor['composite_control']['code'],2)

    def test_missing_definition_and_rejected_marker_are_not_fresh_pass(self):
        definition=self.definition();definition['markers']={}
        self.assertEqual(evaluate_controls(self.report,definition)['composite_control']['code'],-2)
        report=deepcopy(self.report);report['allele_calling']['markers'][0]['status']='rejected'
        self.assertEqual(evaluate_controls(report,self.definition())['composite_control']['code'],-2)

    def test_attach_retains_preliminary_status_and_machine_inputs(self):
        before=deepcopy(self.report)
        result=attach_control_result(self.report,self.definition())
        self.assertEqual(self.report,before);self.assertEqual(result['status'],'tentative')
        self.assertEqual(result['calls'],self.report['calls'])
        self.assertIn('control_definition_sha256',result['settings'])

    def test_control_definition_duplicate_or_missing_booleans_rejected(self):
        with self.assertRaises(ValueError):evaluate_controls(self.report,'{"schema_version":1,"schema_version":1}')
        definition=self.definition();del definition['markers']['ExampleM']['quality_sensor']
        with self.assertRaises(ValueError):evaluate_controls(self.report,definition)

    def test_multiple_ladders_admit_exact_codes_and_mean_offsets(self):
        entries=[]
        for identifier,code,delta in (('first',0,1),('second',.5,3),('NA-admitted',-2,5),('bad',1,100),('uninitialized',-1,100)):
            result=deepcopy(self.report['ladder']);marker=result['markers']['ExampleM']
            marker.update(bin_offsets={key:delta for key in marker['bin_offsets']},min_offset_bp=delta,max_offset_bp=delta)
            entries.append({'ladder_id':identifier,'quality_code':code,'result':result})
        aggregated=aggregate_ladder_offsets(entries,self.kit)
        marker=aggregated['markers']['ExampleM']
        self.assertEqual(marker['bin_offsets'],dict.fromkeys('ABCD',3))
        self.assertEqual(marker['source_ladders'],['first','second','NA-admitted'])
        self.assertEqual(len(marker['excluded']),2)
        self.assertFalse(aggregated['diagnostics']['quality_codes_computed_here'])

    def test_no_admitted_ladder_withholds_offsets_and_does_not_mutate(self):
        entry={'ladder_id':'rejected','quality_code':2,'result':deepcopy(self.report['ladder'])}
        before=deepcopy(entry);result=aggregate_ladder_offsets([entry],self.kit)
        self.assertEqual(result['status'],'rejected');self.assertEqual(result['markers']['ExampleM']['bin_offsets'],{})
        self.assertEqual(entry,before)
        with self.assertRaises(ValueError):aggregate_ladder_offsets([entry,entry],self.kit)


if __name__=='__main__':unittest.main()
