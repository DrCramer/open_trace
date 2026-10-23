from copy import deepcopy
import json
import unittest
from unittest.mock import patch
from genemapper_open.server import analyze
from genemapper_open.str_demo import make_run_demo,make_negative_demo_definitions
from genemapper_open.project_store import ProjectStore
from genemapper_open.project_runs import prepare_run_inputs,create_project_run,revise_project_run,change_run_ladder
from genemapper_open.run_workflow import run_str_batch_workflow


class ProjectRunWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        f=make_run_demo();cls.fixture=f
        sample=analyze(f['sample_bytes'],'sample.fsa',**f['settings'])
        cls.inputs={'schema_version':1,'kit':f['kit'],'ladder_quality_definition':f['ladder_quality_definition'],
            'missing_offset_policy':'reject',
            'ladders':[{'ladder_id':r['ladder_id'],'analysis':analyze(r['data'],r['ladder_id'],**f['settings'])} for r in f['ladders']],
            'samples':[{'sample_key':'Own-Sample','analysis':sample,'marker_quality_definition':f['marker_quality_definition']},
                {'sample_key':'Own-Control','analysis':deepcopy(sample)}]}
        quality,control=make_negative_demo_definitions()
        cls.inputs['samples'][1].update(marker_quality_definition=quality,control_definition=control)

    def setUp(self):self.store=ProjectStore.memory('Own persistent run')
    def tearDown(self):self.store.close()
    def create(self,inputs=None):return create_project_run(self.store,'Own Run',self.inputs if inputs is None else inputs,actor='Tester',reason='Own fixture')

    def test_creates_two_results_with_one_ladder_pass_and_one_call_per_sample(self):
        from genemapper_open.str_workflow import match_ladder,call_sample
        with patch('genemapper_open.str_workflow.match_ladder',wraps=match_ladder) as ladder,patch('genemapper_open.str_workflow.call_sample',wraps=call_sample) as sample:
            created=self.create()
        self.assertEqual(ladder.call_count,3);self.assertEqual(sample.call_count,2)
        self.assertEqual(len(created['outputs']),2)
        reports=[self.store.get_analysis(o['analysis_id'])['result'] for o in created['outputs']]
        self.assertEqual([[c['label'] for c in r['calls']] for r in reports],[list('BC'),list('BC')])
        self.assertEqual(reports[0]['quality']['marker_gq']['markers'][0]['score']['gq'],1)
        self.assertEqual(reports[1]['quality']['marker_gq']['markers'][0]['score']['gq'],0)
        self.assertEqual(len(self.store.list_samples()),2)

    def test_replace_recalculates_all_and_preserves_old_machine_and_review(self):
        created=self.create();old=created['outputs'][0]['analysis_id'];machine=self.store.get_analysis(old)
        self.store.append_review(old,'exclude',target_kind='call',target_id=machine['result']['calls'][0]['id'],actor='Reviewer',reason='Own review')
        event=self.store.review_events(old)
        with patch('genemapper_open.server.analyze',side_effect=AssertionError('No trace reanalysis')):
            changed=change_run_ladder(self.store,created['run_id'],created['revision_id'],'own-a.fsa',
                analysis=self.inputs['ladders'][2]['analysis'],actor='Tester',reason='Replace sized ladder')
        self.assertEqual(len(changed['outputs']),2)
        for before,after in zip(created['outputs'],changed['outputs']):
            self.assertEqual(after['previous_analysis_id'],before['analysis_id'])
            self.assertNotEqual(after['analysis_id'],before['analysis_id'])
            result=self.store.get_analysis(after['analysis_id'])['result']
            self.assertEqual(result['ladder']['markers']['ExampleM']['bin_offsets'],dict.fromkeys('ABCD',-.5))
            self.assertEqual(self.store.review_events(after['analysis_id']),[])
        self.assertEqual(self.store.get_analysis(old),machine);self.assertEqual(self.store.review_events(old),event)
        self.assertEqual(self.store.effective_analysis(old)['run_contexts'][0]['freshness'],'superseded')
        self.assertEqual(self.store.effective_analysis(changed['outputs'][0]['analysis_id'])['run_contexts'][0]['freshness'],'current')

    def test_remove_all_ladders_withholds_calls_then_restore_creates_new_history(self):
        current=self.create()
        for identifier in ('Own-A.fsa','Own-B.fsa','Own-Failed.fsa'):
            current=change_run_ladder(self.store,current['run_id'],current['revision_id'],identifier,remove=True,actor='T',reason='Remove own ladder')
        for output in current['outputs']:
            report=self.store.get_analysis(output['analysis_id'])['result']
            self.assertEqual(report['status'],'rejected');self.assertEqual(report['calls'],[])
            self.assertEqual(report['quality']['run_ladders'],[])
        restored=revise_project_run(self.store,current['run_id'],current['revision_id'],self.inputs,actor='T',reason='Restore inputs as a new revision')
        self.assertEqual(len(self.store.list_run_revisions(current['run_id'])),5)
        self.assertEqual(len(self.store.get_analysis(restored['outputs'][0]['analysis_id'])['result']['calls']),2)

    def test_stale_parent_and_bad_replacement_leave_no_partial_records(self):
        created=self.create();new=change_run_ladder(self.store,created['run_id'],created['revision_id'],'Own-A.fsa',remove=True,actor='T',reason='Remove')
        before=self.store.export_json()
        with self.assertRaises(ValueError):revise_project_run(self.store,new['run_id'],created['revision_id'],self.inputs,actor='T',reason='Stale request')
        with self.assertRaises(ValueError):change_run_ladder(self.store,new['run_id'],new['revision_id'],'Own-B.fsa',analysis={},actor='T',reason='Malformed replacement')
        self.assertEqual(self.store.export_json(),before)

    def test_failed_first_sample_does_not_prevent_valid_sibling_or_requested_qc(self):
        inputs=deepcopy(self.inputs);inputs['samples'][0]['analysis']['sizing']['status']='rejected'
        created=self.create(inputs);a,b=[self.store.get_analysis(o['analysis_id'])['result'] for o in created['outputs']]
        self.assertEqual(a['status'],'rejected');self.assertEqual(a['calls'],[])
        self.assertEqual(a['allele_calling']['diagnostics']['calling_executed'],False)
        self.assertNotEqual(a['quality']['marker_gq']['status'],'evaluated_subset')
        self.assertEqual([c['label'] for c in b['calls']],list('BC'))

    def test_export_import_is_self_contained_without_raw_signals_or_files(self):
        created=self.create();document=self.store.export_json()
        self.assertNotIn('"values":',document);self.assertNotIn('"directory":',document)
        with ProjectStore.from_json(document) as imported:
            revised=revise_project_run(imported,created['run_id'],created['revision_id'],imported.get_run_revision(created['revision_id'])['inputs'],actor='T',reason='Recalculate from retained observations')
            self.assertEqual(len(revised['outputs']),2)
            self.assertEqual(imported.get_run_revision(created['revision_id']),self.store.get_run_revision(created['revision_id']))

    def test_invalid_definitions_and_duplicate_ladders_do_not_create_a_run(self):
        for mutate in (lambda i:i['ladders'].append(deepcopy(i['ladders'][0])),
            lambda i:i['samples'][0].update(marker_quality_definition={}),
            lambda i:i['samples'][1]['control_definition']['markers']['ExampleM'].update(quality_sensor='false')):
            inputs=deepcopy(self.inputs);mutate(inputs)
            with self.assertRaises(ValueError):self.create(inputs)
            self.assertEqual(self.store.list_runs(),[]);self.assertEqual(self.store.list_analyses(),[])

    def test_atomic_failure_after_analyses_rolls_back_every_new_record(self):
        before=self.store.export_json()
        with patch.object(self.store,'append_run_revision',side_effect=ValueError('Commit boundary')):
            with self.assertRaises(ValueError):self.create()
        self.assertEqual(self.store.export_json(),before)

    def test_inputs_not_modified_and_empty_sample_revision_is_explicit(self):
        before=deepcopy(self.inputs);created=self.create();self.assertEqual(self.inputs,before)
        inputs=self.store.get_run_revision(created['revision_id'])['inputs'];inputs['samples']=[]
        revised=revise_project_run(self.store,created['run_id'],created['revision_id'],inputs,actor='T',reason='Remove registered samples')
        self.assertEqual(revised['outputs'],[])
        self.assertEqual(self.store.effective_analysis(created['outputs'][0]['analysis_id'])['run_contexts'][0]['freshness'],'removed')


if __name__=='__main__':unittest.main()
