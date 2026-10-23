from copy import deepcopy
import json
import unittest
from genemapper_open.server import analyze
from genemapper_open.str_demo import make_str_demo,make_ladder_quality_demo_definition,make_cross_check_demo
from genemapper_open.str_workflow import run_str_workflow,analyze_str_files,compact_analysis
from genemapper_open.project_api import project_action


class LadderQualityWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture=make_str_demo();x=cls.fixture
        cls.sample=analyze(x['sample_bytes'],'sample.fsa',**x['settings'])
        cls.ladder=analyze(x['ladder_bytes'],'ladder.fsa',**x['settings'])

    def run_qc(self,definition=None,ladder=None,kit=None):
        return run_str_workflow(self.sample,self.ladder if ladder is None else ladder,
            self.fixture['kit'] if kit is None else kit,
            ladder_quality_definition=make_ladder_quality_demo_definition() if definition is None else definition)

    def test_actual_fsa_to_quality_admitted_offsets_and_tentative_sample_calls(self):
        x=self.fixture
        report=analyze_str_files(x['sample_bytes'],x['ladder_bytes'],x['kit'],settings=x['settings'],
                                ladder_quality_definition=make_ladder_quality_demo_definition())
        quality=report['quality']['ladder_gq']
        self.assertEqual(report['status'],'tentative');self.assertTrue(quality['gate_passed'])
        self.assertEqual(quality['al_subset_code'],0);self.assertFalse(quality['full_original_al'])
        self.assertEqual([c['label'] for c in report['calls']],['B','C'])
        self.assertEqual([c['name'] for c in quality['markers'][0]['calls']],list('ABCD'))
        self.assertTrue(report['provenance']['ladder_quality_applied'])

    def test_failed_quality_withholds_offsets_and_all_sample_calls_despite_accepted_matching(self):
        report=self.run_qc(make_ladder_quality_demo_definition(reject=True))
        quality=report['quality']['ladder_gq'];marker=report['ladder']['markers']['ExampleM']
        self.assertEqual(report['status'],'rejected');self.assertEqual(report['calls'],[])
        self.assertEqual(marker['matching_status_before_quality'],'accepted')
        self.assertEqual(len(marker['matched']),4);self.assertEqual(marker['bin_offsets'],{})
        self.assertIsNone(marker['min_offset_bp']);self.assertFalse(quality['gate_passed'])
        self.assertEqual(quality['al_subset_code'],2)

    def test_missing_recipe_or_base_quality_cannot_admit_offsets(self):
        fixture,_=make_cross_check_demo()
        definition=make_ladder_quality_demo_definition();recipe=definition['markers'].pop('ExampleM')
        definition['markers']={'AMEL':recipe}
        report=self.run_qc(definition,kit=fixture['kit'])
        self.assertEqual(report['quality']['ladder_gq']['status'],'partial')
        self.assertEqual(report['calls'],[])
        self.assertIsNone(report['quality']['ladder_gq']['al_subset_code'])
        ladder=deepcopy(self.ladder);ladder['sizing'].pop('base_quality')
        self.assertFalse(self.run_qc(ladder=ladder)['quality']['ladder_gq']['gate_passed'])

    def test_known_offscale_or_spike_cannot_be_hidden_by_zero_recipe_metric(self):
        ladder=deepcopy(self.ladder)
        peaks=next(c for c in ladder['channels'] if c['tag']=='DATA1')['peaks']
        peaks[1]['offscaled']=True
        quality=self.run_qc(ladder=ladder)['quality']['ladder_gq']
        self.assertTrue(quality['markers'][0]['observed_flags']['offscale'])
        self.assertEqual(quality['markers'][0]['score']['recipe']['offscale_metric'],1)
        self.assertFalse(quality['gate_passed'])
        ladder=deepcopy(self.ladder)
        peaks=next(c for c in ladder['channels'] if c['tag']=='DATA1')['peaks']
        extra=deepcopy(peaks[1]);extra.update(scan=extra['scan']+5,size_bp=extra['size_bp']+.25,spike=True,height=10)
        peaks.insert(2,extra)
        quality=self.run_qc(ladder=ladder)['quality']['ladder_gq']
        self.assertTrue(quality['markers'][0]['observed_flags']['spike'])
        self.assertEqual(quality['markers'][0]['score']['recipe']['spike_metric'],1)
        self.assertFalse(quality['gate_passed'])

    def test_missing_offscale_observation_is_unavailable(self):
        ladder=deepcopy(self.ladder)
        next(c for c in ladder['channels'] if c['tag']=='DATA1')['peaks'][0].pop('offscaled')
        quality=self.run_qc(ladder=ladder)['quality']['ladder_gq']
        self.assertEqual(quality['markers'][0]['reason'],'ladder_offscale_state_missing')
        self.assertFalse(quality['gate_passed'])

    def test_sq_category_only_admits_pass_and_does_not_replace_numeric_sq(self):
        for sq,category in ((1,0),(.75,0),(.5,1),(.25,1),(.24,2)):
            definition=make_ladder_quality_demo_definition();definition['ladder_sq']={'source':'explicit','score':sq}
            report=self.run_qc(definition);quality=report['quality']['ladder_gq']
            self.assertEqual(quality['ladder_sq'],sq);self.assertEqual(quality['al_subset_code'],category)
            self.assertEqual(quality['gate_passed'],category==0)
            self.assertEqual(bool(report['calls']),category==0)

    def test_one_failed_marker_blocks_the_whole_ladder(self):
        fixture,_=make_cross_check_demo()
        definition=make_ladder_quality_demo_definition();recipe=definition['markers'].pop('ExampleM')
        definition['markers']={'AMEL':deepcopy(recipe),'ExampleY':deepcopy(recipe)}
        definition['markers']['ExampleY']['offscale_metric']=1
        report=self.run_qc(definition,kit=fixture['kit'])
        self.assertEqual([r['score']['gq'] for r in report['quality']['ladder_gq']['markers']],[1,0])
        self.assertEqual(report['calls'],[])
        self.assertTrue(all(r['bin_offsets']=={} for r in report['ladder']['markers'].values()))

    def test_no_requested_qc_retains_offsets_without_claiming_quality(self):
        report=run_str_workflow(self.sample,self.ladder,self.fixture['kit'])
        self.assertNotIn('ladder_gq',report['quality'])
        self.assertFalse(report['provenance']['ladder_quality_applied'])
        self.assertEqual([c['label'] for c in report['calls']],['B','C'])

    def test_strict_definition_duplicates_unknown_fields_and_marker_names(self):
        with self.assertRaises(ValueError):self.run_qc('{"schema_version":1,"schema_version":1}')
        for key,value in (('schema_version',True),('calling_parameters_source','automatic'),('extra',0)):
            definition=make_ladder_quality_demo_definition();definition[key]=value
            with self.assertRaises(ValueError):self.run_qc(definition)
        definition=make_ladder_quality_demo_definition();definition['markers']['Typo']=definition['markers'].pop('ExampleM')
        with self.assertRaises(ValueError):self.run_qc(definition)

    def test_inputs_are_immutable_and_compact_analysis_matches_full(self):
        before=deepcopy((self.sample,self.ladder,self.fixture['kit']))
        full=self.run_qc()
        compact=run_str_workflow(compact_analysis(self.sample),compact_analysis(self.ladder),self.fixture['kit'],
            ladder_quality_definition=make_ladder_quality_demo_definition())
        self.assertEqual(full,compact);self.assertEqual(before,(self.sample,self.ladder,self.fixture['kit']))
        self.assertNotIn('"values":',json.dumps(full,allow_nan=False))

    def test_passed_and_rejected_qc_survive_project_roundtrip(self):
        state=project_action({'action':'new','name':'Ladder gates'})
        reports=[self.run_qc(),self.run_qc(make_ladder_quality_demo_definition(reject=True))]
        for report in reports:
            state=project_action({'action':'save','document':state['document'],'report':report})
        imported=project_action({'action':'import','document':state['document']})
        self.assertEqual(len(imported['analyses']),2)
        self.assertEqual(imported['effective']['machine_result']['quality'],reports[-1]['quality'])
        self.assertEqual(imported['effective']['machine_result']['calls'],[])
