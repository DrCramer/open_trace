from copy import deepcopy
import json
import unittest
from genemapper_open.server import analyze
from genemapper_open.str_demo import make_str_demo
from genemapper_open.str_workflow import analyze_str_files, compact_analysis, run_str_workflow
from genemapper_open.project_api import project_action


class StrWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture=make_str_demo()
        cls.sample=analyze(cls.fixture['sample_bytes'],'sample.fsa',**cls.fixture['settings'])
        cls.ladder=analyze(cls.fixture['ladder_bytes'],'ladder.fsa',**cls.fixture['settings'])

    def run_workflow(self,sample=None,ladder=None,kit=None,**kwargs):
        return run_str_workflow(self.sample if sample is None else sample,
            self.ladder if ladder is None else ladder,self.fixture['kit'] if kit is None else kit,**kwargs)

    def test_generated_files_produce_known_labels_and_offsets(self):
        result=analyze_str_files(self.fixture['sample_bytes'],self.fixture['ladder_bytes'],
                                 self.fixture['kit'],settings=self.fixture['settings'])
        self.assertEqual(result['status'],'tentative',result['reasons'])
        self.assertEqual([c['label'] for c in result['calls']],['B','C'])
        for delta in result['ladder']['markers']['ExampleM']['bin_offsets'].values():
            self.assertAlmostEqual(delta,-.3,places=8)
        self.assertFalse(result['provenance']['input_reports_are_caller_supplied'])
        self.assertTrue(all(c['review_required'] for c in result['calls']))

    def test_compact_report_preserves_results_without_trace_payloads(self):
        full=self.run_workflow()
        compact=self.run_workflow(compact_analysis(self.sample),compact_analysis(self.ladder))
        self.assertEqual(full,compact)
        self.assertEqual(len({c['id'] for c in compact['calls']}),2)
        for banned in ('values','channels','directory'):
            self.assertNotIn('"'+banned+'":',json.dumps(compact,allow_nan=False))

    def test_rejected_sizing_withholds_every_call_and_offset(self):
        for role in ('sample','ladder'):
            for field,value in (('status','rejected'),('calibration_applied',False),('mode','manual')):
                sample,ladder=deepcopy(self.sample),deepcopy(self.ladder)
                (sample if role=='sample' else ladder)['sizing'][field]=value
                result=self.run_workflow(sample,ladder)
                self.assertEqual(result['status'],'rejected')
                self.assertEqual(result['calls'],[])
                self.assertIsNone(result['ladder'])

    def test_different_settings_or_channel_group_cannot_share_offsets(self):
        ladder=deepcopy(self.ladder)
        ladder['sizing']['expected_bp']=[40,70,110,145,195,261]
        self.assertEqual(self.run_workflow(ladder=ladder)['calls'],[])
        ladder=deepcopy(self.ladder)
        ladder['peak_settings']['threshold']=101
        self.assertEqual(self.run_workflow(ladder=ladder)['status'],'rejected')
        ladder=deepcopy(self.ladder)
        ladder['peak_settings']['selected_data_group']='analyzed'
        self.assertEqual(self.run_workflow(ladder=ladder)['status'],'rejected')

    def test_missing_kit_channel_or_missing_ladder_peaks_withholds_labels(self):
        sample=deepcopy(self.sample)
        sample['channels']=[c for c in sample['channels'] if c['tag']!='DATA1']
        self.assertEqual(self.run_workflow(sample=sample)['calls'],[])
        ladder=deepcopy(self.ladder)
        next(c for c in ladder['channels'] if c['tag']=='DATA1')['peaks']=[]
        self.assertEqual(self.run_workflow(ladder=ladder)['status'],'rejected')

    def test_reference_channel_cannot_be_a_marker(self):
        kit=deepcopy(self.fixture['kit']);kit['markers'][0]['channel']='DATA106'
        with self.assertRaises(ValueError):self.run_workflow(kit=kit)

    def test_different_algorithm_versions_cannot_share_ladder(self):
        ladder=deepcopy(self.ladder);ladder['analysis_version']='OpenTrace/older'
        result=self.run_workflow(ladder=ladder)
        self.assertEqual(result['status'],'rejected');self.assertEqual(result['calls'],[])

    def test_nonfinite_or_duplicate_analysis_input_rejected(self):
        sample=deepcopy(self.sample)
        sample['channels'][0]['peaks'][0]['size_bp']=float('nan')
        with self.assertRaises(ValueError):self.run_workflow(sample=sample)
        sample=deepcopy(self.sample);sample['channels'].append(sample['channels'][0])
        with self.assertRaises(ValueError):self.run_workflow(sample=sample)

    def test_original_inputs_are_not_modified(self):
        source=(deepcopy(self.sample),deepcopy(self.ladder),deepcopy(self.fixture['kit']))
        self.run_workflow()
        self.assertEqual(source,(self.sample,self.ladder,self.fixture['kit']))

    def test_project_roundtrip_review_and_undo_preserve_machine_result(self):
        report=self.run_workflow()
        state=project_action({'action':'new','name':'Synthetic review'})
        state=project_action({'action':'save','document':state['document'],'report':report})
        original=deepcopy(state['effective']['machine_result'])
        call=report['calls'][0]
        state=project_action({'action':'review','document':state['document'],
            'analysis_id':state['selected_analysis_id'],'review_action':'change-label',
            'actor':'Test reviewer','reason':'Synthetic correction','target_kind':'call',
            'target_id':call['id'],'value':'Example replacement'})
        overlay=next(t for t in state['effective']['review_targets'] if t['target_id']==call['id'])
        self.assertEqual(overlay['label'],'Example replacement')
        self.assertEqual(state['effective']['machine_result'],original)
        state=project_action({'action':'undo','document':state['document'],
            'analysis_id':state['selected_analysis_id'],'event_id':state['events'][0]['event_id'],
            'actor':'Test reviewer','reason':'Restore original'})
        state=project_action({'action':'import','document':state['document']})
        overlay=next(t for t in state['effective']['review_targets'] if t['target_id']==call['id'])
        self.assertEqual(overlay['label'],'B')
        self.assertEqual(state['effective']['machine_result'],original)
        self.assertEqual(len(state['events']),2)

    def test_project_invalid_actor_or_digest_cannot_change_exported_state(self):
        state=project_action({'action':'new','name':'Synthetic review'})
        state=project_action({'action':'save','document':state['document'],'report':self.run_workflow()})
        document=state['document']
        with self.assertRaises(ValueError):
            project_action({'action':'review','document':document,'analysis_id':state['selected_analysis_id'],
                'review_action':'note','actor':'','reason':'required','value':'note'})
        self.assertEqual(project_action({'action':'import','document':document})['events'],[])
        payload=json.loads(document);payload['analyses'][0]['result']['calls'][0]['label']='tampered'
        with self.assertRaises(ValueError):project_action({'action':'import','document':json.dumps(payload)})


if __name__=='__main__':unittest.main()
