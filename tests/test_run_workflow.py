from copy import deepcopy
import json
import unittest
from unittest.mock import patch
from genemapper_open.server import analyze
from genemapper_open.str_demo import make_run_demo
from genemapper_open.str_workflow import compact_analysis
from genemapper_open.run_workflow import run_str_run_workflow,analyze_str_run_files
from genemapper_open.project_api import project_action


class RunWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture=make_run_demo();f=cls.fixture
        cls.sample=analyze(f['sample_bytes'],'sample.fsa',**f['settings'])
        cls.ladders=[{'ladder_id':r['ladder_id'],'analysis':analyze(r['data'],r['ladder_id'],**f['settings'])} for r in f['ladders']]

    def run_case(self,ladders=None,sample=None):
        return run_str_run_workflow(sample or self.sample,self.ladders if ladders is None else ladders,
            self.fixture['kit'],ladder_quality_definition=self.fixture['ladder_quality_definition'])

    def test_admitted_ladders_mean_excludes_failed_and_calls_sample_once(self):
        from genemapper_open.allele_calling import call_sample
        with patch('genemapper_open.str_workflow.call_sample',wraps=call_sample) as caller:
            r=self.run_case()
        self.assertEqual(caller.call_count,1)
        self.assertEqual([s['quality_code'] for s in r['quality']['run_ladders']],[0,0,2])
        self.assertEqual([c['label'] for c in r['calls']],list('BC'))
        keys=r['ladder']['markers']['ExampleM']['per_key']
        for key in [*keys['bins'].values(),keys['virtual_min'],keys['virtual_max']]:
            self.assertAlmostEqual(key['offset_bp'],-.3)
            self.assertEqual([c['ladder_id'] for c in key['contributors']],['Own-A.fsa','Own-B.fsa'])
            self.assertEqual([c['ladder_id'] for c in key['excluded']],['Own-Failed.fsa'])

    def test_call_thresholds_follow_id_x_zygosity_floors(self):
        def run(**floors):
            return run_str_run_workflow(self.sample,self.ladders,self.fixture['kit'],
                ladder_quality_definition=self.fixture['ladder_quality_definition'],**floors)
        unfiltered=run(homozygous_min_height=0,heterozygous_min_height=0)
        self.assertTrue(unfiltered['calls'])
        # Demo sample is heterozygous (two called alleles), so only the heterozygous floor applies to it.
        het_blocked=run(homozygous_min_height=0,heterozygous_min_height=1e9)
        self.assertEqual(het_blocked['calls'],[])
        hom_blocked=run(homozygous_min_height=1e9,heterozygous_min_height=0)
        self.assertEqual([c['label'] for c in hom_blocked['calls']],[c['label'] for c in unfiltered['calls']])
        self.assertEqual(het_blocked['ladder'],unfiltered['ladder'])
        default=run()
        self.assertEqual(default['settings']['call_thresholds'],{'homozygous':200.0,'heterozygous':100.0})
        for bad in (-1,float('nan'),True,'200'):
            with self.subTest(bad=bad),self.assertRaises(ValueError):
                run(homozygous_min_height=bad)
            with self.subTest(bad=bad),self.assertRaises(ValueError):
                run(heterozygous_min_height=bad)

    def test_reanalysis_replaces_same_filename_case_insensitively(self):
        rows=deepcopy(self.ladders[:2]);rows[1]['ladder_id']='oWN-a.FSA'
        r=self.run_case(rows)
        self.assertAlmostEqual(r['ladder']['markers']['ExampleM']['bin_offsets']['A'],-.5)
        self.assertEqual(r['ladder']['diagnostics']['entry_history'][1]['replaced_ladder_id'],'Own-A.fsa')

    def test_failed_reanalysis_withdraws_old_offsets_and_withholds_calls(self):
        rows=deepcopy([self.ladders[0],self.ladders[2]]);rows[1]['ladder_id']='own-a.fsa'
        r=self.run_case(rows)
        self.assertEqual(r['status'],'rejected');self.assertEqual(r['calls'],[])
        self.assertEqual(r['ladder']['markers']['ExampleM']['bin_offsets'],{})
        self.assertEqual(r['quality']['run_ladders'][1]['quality_code'],2)

    def test_incompatible_latest_analysis_withdraws_old_contribution(self):
        rows=deepcopy([self.ladders[0],self.ladders[0]]);rows[1]['analysis']['peak_settings']['threshold']=101
        r=self.run_case(rows)
        self.assertIsNone(r['quality']['run_ladders'][1]['quality_code'])
        self.assertEqual(r['calls'],[])
        self.assertIn('settings differ',r['quality']['run_ladders'][1]['reasons'][0])

    def test_unavailable_quality_never_manufactures_na_or_override_codes(self):
        rows=deepcopy(self.ladders[:1]);rows[0]['analysis']['sizing']['base_quality']=None
        r=self.run_case(rows)
        self.assertIsNone(r['quality']['run_ladders'][0]['quality_code'])
        self.assertEqual(r['calls'],[])
        self.assertTrue(all(c['quality_code'] is None for c in r['ladder']['state']['contributions']))

    def test_compact_reports_and_project_roundtrip_preserve_run_context(self):
        r=self.run_case();compact=self.run_case([{'ladder_id':r['ladder_id'],'analysis':compact_analysis(r['analysis'])} for r in self.ladders],compact_analysis(self.sample))
        self.assertEqual(r,compact)
        state=project_action({'action':'new','name':'Own multi-ladder run'})
        state=project_action({'action':'save','document':state['document'],'report':r})
        imported=project_action({'action':'import','document':state['document']})
        self.assertEqual(imported['effective']['machine_result']['quality'],r['quality'])
        self.assertEqual(imported['effective']['machine_result']['ladder'],r['ladder'])
        self.assertNotIn('"values":',json.dumps(r,allow_nan=False))

    def test_file_workflow_binds_newly_computed_analyses(self):
        f=self.fixture;r=analyze_str_run_files(f['sample_bytes'],f['ladders'],f['kit'],settings=f['settings'],
            ladder_quality_definition=f['ladder_quality_definition'])
        self.assertFalse(r['provenance']['input_reports_are_caller_supplied'])
        expected=self.run_case();expected['provenance']['input_reports_are_caller_supplied']=False
        self.assertEqual(r,expected)

    def test_required_explicit_quality_and_bounded_ladder_list(self):
        for rows in ([],self.ladders*43,[{'ladder_id':'x','analysis':self.sample,'quality_code':-2}]):
            with self.assertRaises(ValueError):self.run_case(rows)
        with self.assertRaises(ValueError):
            run_str_run_workflow(self.sample,self.ladders,self.fixture['kit'],ladder_quality_definition=None)

    def test_reference_bins_genotype_without_passing_ladders(self):
        f=self.fixture
        r=run_str_run_workflow(self.sample,[],f['kit'],ladder_quality_definition=f['ladder_quality_definition'],
            missing_offset_policy='reference_bins')
        self.assertEqual(r['ladder']['status'],'rejected')
        self.assertEqual(r['settings']['missing_offset_policy'],'reference_bins')
        marker=r['allele_calling']['markers'][0]
        self.assertTrue(marker['offsets']['reference_bins'])
        failed=[x for x in self.ladders if 'Failed' in x['ladder_id']]
        r2=run_str_run_workflow(self.sample,failed,f['kit'],ladder_quality_definition=f['ladder_quality_definition'],
            missing_offset_policy='reference_bins')
        self.assertEqual(r2['quality']['run_ladders'][0]['quality_code'],2)
        self.assertTrue(r2['allele_calling']['markers'][0]['offsets']['reference_bins'])
        with self.assertRaises(ValueError):
            run_str_run_workflow(self.sample,[],f['kit'],ladder_quality_definition=f['ladder_quality_definition'],
                missing_offset_policy='forward_fill')

    def test_inputs_are_not_modified(self):
        before=deepcopy((self.sample,self.ladders,self.fixture));self.run_case()
        self.assertEqual((self.sample,self.ladders,self.fixture),before)

    def split_kit(self):
        kit=deepcopy(self.fixture['kit']);prototype=kit['markers'][0];markers=[]
        for name,low,high,start in (('First',160,180,0),('Second',180,200,2)):
            m=deepcopy(prototype);m.update(name=name,min_bp=low,max_bp=high,
                bins=m['bins'][start:start+2],ladder_labels=m['ladder_labels'][start:start+2]);markers.append(m)
        kit['markers']=markers;q=deepcopy(self.fixture['ladder_quality_definition']);recipe=q['markers'].pop('ExampleM')
        q['markers']={m['name']:deepcopy(recipe) for m in markers}
        return kit,q

    def test_later_ladder_windows_consume_prior_contributors_plus_local_offset(self):
        kit,recipe=self.split_kit()
        r=run_str_run_workflow(self.sample,self.ladders[:2],kit,ladder_quality_definition=recipe)
        self.assertEqual([c['label'] for c in r['calls']],list('BC'))
        stage=r['quality']['run_ladders'][1];sources=stage['matching']['markers']['Second']['diagnostics']['previous_offset_sources']
        self.assertEqual(sources['value_count'],2)
        self.assertEqual(sources['run_contributors'][0]['ladder_id'],'Own-A.fsa')
        self.assertAlmostEqual(sources['offset_bp'],-.3)
        self.assertAlmostEqual(stage['matching']['markers']['Second']['window']['min_bp'],180.3)

    def test_reanalysis_withdraws_its_previous_values_before_window_matching(self):
        kit,recipe=self.split_kit();rows=deepcopy(self.ladders[:2]);rows[1]['ladder_id']='own-a.fsa'
        r=run_str_run_workflow(self.sample,rows,kit,ladder_quality_definition=recipe)
        sources=r['quality']['run_ladders'][1]['matching']['markers']['Second']['diagnostics']['previous_offset_sources']
        self.assertEqual(sources['run_contributors'],[]);self.assertEqual(sources['value_count'],1)
        self.assertAlmostEqual(sources['offset_bp'],-.5)


if __name__=='__main__':unittest.main()
