import json
import re
import subprocess
import sys
import tempfile
import unittest
import urllib.request
import urllib.error


class StrHttpTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server_log=tempfile.TemporaryFile(mode='w+',encoding='utf-8')
        cls.process=subprocess.Popen([sys.executable,'-u','-m','genemapper_open','serve','--port','0'],
            stdout=subprocess.PIPE,stderr=cls.server_log,text=True,encoding='utf-8')
        cls.base=re.search(r'http://127\.0\.0\.1:\d+/',cls.process.stdout.readline()).group(0)
        with urllib.request.urlopen(cls.base,timeout=5) as response:
            cls.token=re.search(r"const TOKEN='([^']+)'",response.read().decode()).group(1)

    @classmethod
    def tearDownClass(cls):
        cls.process.terminate();cls.process.wait(timeout=5)
        cls.process.stdout.close();cls.server_log.close()

    def post(self,path,payload,headers=None,expected=200):
        request=urllib.request.Request(self.base+path,data=json.dumps(payload).encode(),
            headers={'X-Session-Token':self.token,'Content-Type':'application/json',**(headers or {})},method='POST')
        try:response=urllib.request.urlopen(request,timeout=15)
        except urllib.error.HTTPError as error:response=error
        with response:
            self.assertEqual(response.code,expected)
            return json.load(response)

    def test_routes_link_demo_str_and_project_review(self):
        demo=self.post('api/demo-str',{})
        report=self.post('api/str',{'sample':demo['sample'],'ladder':demo['ladder'],'kit':demo['kit']})
        self.assertEqual([c['label'] for c in report['calls']],['B','C'])
        state=self.post('api/project',{'action':'new','name':'HTTP synthetic'})
        state=self.post('api/project',{'action':'save','document':state['document'],'report':report})
        state=self.post('api/project',{'action':'review','document':state['document'],
            'analysis_id':state['selected_analysis_id'],'review_action':'exclude','target_kind':'call',
            'target_id':report['calls'][0]['id'],'actor':'Test','reason':'Synthetic boundary'})
        overlay=next(t for t in state['effective']['review_targets'] if t['target_kind']=='call')
        self.assertFalse(overlay['accepted'])
        self.assertEqual(state['effective']['machine_result']['calls'][0]['label'],'B')

    def test_invalid_requests_do_not_break_server(self):
        self.assertIn('error',self.post('api/str',[],expected=400))
        self.assertIn('error',self.post('api/project',{'action':'import','document':'{}'},expected=400))
        self.assertIn('error',self.post('api/demo-str',{},headers={'X-Session-Token':'wrong'},expected=403))
        self.assertIn('error',self.post('api/demo-str',{},headers={'Origin':'https://example.com'},expected=403))
        self.assertEqual(self.post('api/project',{'action':'new','name':'Still running'})['analyses'],[])

    def test_companion_morphology_demo_and_project_keep_measured_flags(self):
        demo=self.post('api/demo-str?variant=morphology',{})
        plain=self.post('api/demo-str?variant=morphology-unrequested',{})
        self.assertEqual(demo['sample']['source_sha256'],plain['sample']['source_sha256'])
        self.assertEqual(demo['sample']['sizing'],plain['sample']['sizing'])
        self.assertEqual([c['label'] for c in plain['report']['calls']],['B','C'])
        self.assertEqual([c['label'] for c in demo['report']['calls']],['C'])
        self.assertEqual(demo['spike_definition']['schema_version'],3)
        self.assertEqual(demo['report']['peaks'][0]['spike_features']['jag_count'],1)
        state=self.post('api/project',{'action':'new','name':'Companion morphology'})
        state=self.post('api/project',{'action':'save','document':state['document'],'report':demo['report']})
        self.assertEqual(state['effective']['machine_result']['peaks'],demo['report']['peaks'])
        self.assertEqual(state['effective']['machine_result']['provenance'],demo['report']['provenance'])

    def test_true_peak_restoration_and_exclusion_reach_http_project(self):
        restored=self.post('api/demo-str?variant=true-peak',{})
        excluded=self.post('api/demo-str?variant=true-peak-excluded',{})
        self.assertEqual(restored['sample']['source_sha256'],excluded['sample']['source_sha256'])
        self.assertEqual(restored['sample']['sizing'],excluded['sample']['sizing'])
        self.assertEqual([c['label'] for c in restored['report']['calls']],['B','C'])
        self.assertEqual([c['label'] for c in excluded['report']['calls']],['C'])
        self.assertEqual(restored['report']['peaks'][0]['true_peak_restoration']['status'],'restored')
        state=self.post('api/project',{'action':'new','name':'True-peak history'})
        state=self.post('api/project',{'action':'save','document':state['document'],'report':restored['report']})
        self.assertEqual(state['effective']['machine_result']['peaks'],restored['report']['peaks'])

    def test_control_route_records_failure_without_changing_tentative_calls(self):
        demo=self.post('api/demo-str',{})
        definition={'schema_version':1,'sample_type':'Positive Control','markers':{
            'ExampleM':{'expected_alleles':['A','B'],'quality_sensor':False,'is_y_marker':False}}}
        report=self.post('api/controls',{'report':demo['report'],'definition':definition})
        self.assertEqual(report['quality']['controls']['composite_control']['code'],2)
        self.assertEqual(report['calls'],demo['report']['calls']);self.assertEqual(report['status'],'tentative')
        state=self.post('api/project',{'action':'new','name':'Control provenance'})
        state=self.post('api/project',{'action':'save','document':state['document'],'report':report})
        self.assertEqual(state['effective']['machine_result']['quality'],report['quality'])

    def test_marker_quality_route_preserves_control_dependency_and_call_labels(self):
        demo=self.post('api/demo-str',{})
        report=self.post('api/marker-quality',{'report':demo['report'],'definition':demo['marker_quality_definition']})
        self.assertEqual(report['quality']['marker_gq']['sample_category'],0)
        self.assertEqual(report['calls'],demo['report']['calls'])
        self.assertEqual(report['quality']['marker_gq']['markers'][0]['call_scores'][0]['after_sample_sq'],1)
        self.assertIn('error',self.post('api/marker-quality',{'report':report,'definition':'{"schema_version":1,"schema_version":1}'},expected=400))

    def test_negative_demo_and_schema2_annotation_preserve_confidence_on_gq_failure(self):
        demo=self.post('api/demo-str?variant=negative',{})
        quality=demo['report']['quality']['marker_gq']
        self.assertEqual(quality['markers'][0]['score']['gq'],0)
        self.assertEqual([c['after_sample_sq'] for c in quality['markers'][0]['call_scores']],[1,1])
        report=self.post('api/marker-quality',{'report':demo['report'],'definition':demo['marker_quality_definition']})
        self.assertEqual(report,demo['report'])
        changed=demo['control_definition'];changed['markers']['ExampleM'].update(quality_sensor=True,expected_alleles=['B','C'])
        stale=self.post('api/controls',{'report':report,'definition':changed})
        self.assertEqual(stale['quality']['marker_gq']['status'],'stale')
        refreshed=self.post('api/marker-quality',{'report':stale,'definition':demo['marker_quality_definition']})
        self.assertEqual(refreshed['quality']['marker_gq']['sample_category'],0)

    def test_amel_demo_route_and_project_keep_plugin_result_and_call_confidence(self):
        demo=self.post('api/demo-str?variant=amel',{})
        report=demo['report'];quality=report['quality']['marker_gq']
        self.assertTrue(quality['amel_cross_check']['mismatch'])
        self.assertEqual([m['score']['gq'] for m in quality['markers']],[.5,.25])
        self.assertEqual([c['after_sample_sq'] for c in quality['markers'][0]['call_scores']],[1])
        state=self.post('api/project',{'action':'new','name':'AMEL HTTP'})
        state=self.post('api/project',{'action':'save','document':state['document'],'report':report})
        self.assertEqual(state['effective']['machine_result']['quality'],report['quality'])
        self.assertIn('error',self.post('api/demo-str?variant=unknown',{},expected=400))

    def test_ladder_quality_gate_routes_hold_back_labels_after_matching_success(self):
        demo=self.post('api/demo-str?variant=ladder',{})
        self.assertTrue(demo['report']['quality']['ladder_gq']['gate_passed'])
        definition=demo['ladder_quality_definition'];definition['markers']['ExampleM']['min_height_ratio']=1
        rejected=self.post('api/str',{'sample':demo['sample'],'ladder':demo['ladder'],'kit':demo['kit'],
                                    'ladder_quality_definition':definition})
        self.assertEqual(rejected['calls'],[])
        self.assertEqual(rejected['ladder']['markers']['ExampleM']['matching_status_before_quality'],'accepted')
        self.assertEqual(rejected['quality']['ladder_gq']['al_subset_code'],2)
        self.assertIn('error',self.post('api/str',{'sample':demo['sample'],'ladder':demo['ladder'],'kit':demo['kit'],
            'ladder_quality_definition':'{"schema_version":1,"schema_version":1}'},expected=400))

    def test_rejected_ladder_quality_demo_project_retains_rejection_and_empty_calls(self):
        demo=self.post('api/demo-str?variant=ladder-rejected',{})
        report=demo['report'];self.assertEqual(report['status'],'rejected');self.assertEqual(report['calls'],[])
        state=self.post('api/project',{'action':'new','name':'Ladder QC HTTP'})
        state=self.post('api/project',{'action':'save','document':state['document'],'report':report})
        self.assertEqual(state['effective']['machine_result']['quality'],report['quality'])
        self.assertEqual(state['effective']['machine_result']['ladder'],report['ladder'])

    def test_automatic_spike_demo_excludes_a_before_calls_and_affects_gq(self):
        demo=self.post('api/demo-str?variant=spike',{})
        report=demo['report']
        self.assertEqual([c['label'] for c in report['calls']],['B','C'])
        self.assertEqual(sum(p.get('spike',False) for p in report['peaks']),1)
        self.assertEqual(report['quality']['marker_gq']['markers'][0]['score']['gq'],.5)
        plain=self.post('api/demo-str?variant=spike-unrequested',{})
        self.assertEqual([c['label'] for c in plain['report']['calls']],['A','B','C'])
        self.assertEqual(plain['report']['source_sha256'],report['source_sha256'])
        self.assertEqual(plain['sample']['sizing'],demo['sample']['sizing'])

    def test_low_spike_adjustments_change_gq_without_changing_calls_or_sizing(self):
        initial=self.post('api/demo-str?variant=spike-low-initial',{})
        adjusted=self.post('api/demo-str?variant=spike-low-adjusted',{})
        first,last=initial['report'],adjusted['report']
        self.assertEqual(first['source_sha256'],last['source_sha256'])
        self.assertEqual(first['sizing'],last['sizing'])
        self.assertEqual(first['calls'],last['calls'])
        self.assertEqual(first['quality']['marker_gq']['markers'][0]['score']['gq'],.5)
        self.assertEqual(last['quality']['marker_gq']['markers'][0]['score']['gq'],1)
        self.assertTrue(first['peaks'][0]['spike']);self.assertFalse(last['peaks'][0]['spike'])
        replay=self.post('api/str',{'sample':adjusted['sample'],'ladder':adjusted['ladder'],'kit':adjusted['kit']})
        self.assertEqual(replay['peaks'],last['peaks'])
        self.assertEqual(replay['allele_calling'],last['allele_calling'])

    def test_post_sizing_sq_updates_confidence_and_preserves_calibration(self):
        check=self.post('api/demo-str?variant=sq-check',{})
        failed=self.post('api/demo-str?variant=sq-fail',{})
        for demo,expected in ((check,.5),(failed,.2)):
            report=demo['report'];post=report['sizing']['post_quality']
            self.assertAlmostEqual(post['score'],expected)
            self.assertEqual([c['label'] for c in report['calls']],['B','C'])
            self.assertEqual(len(demo['sample']['sizing']['anchors']),6)
            self.assertTrue(demo['sample']['sizing']['calibration_applied'])
            self.assertAlmostEqual(report['quality']['marker_gq']['markers'][0]['call_scores'][0]['after_sample_sq'],expected)
        gated=self.post('api/demo-str?variant=sq-ladder-fail',{})
        self.assertEqual(gated['report']['calls'],[])
        self.assertFalse(gated['report']['quality']['ladder_gq']['gate_passed'])

    def test_manual_review_marks_gq_stale_and_undo_restores_without_recalculation(self):
        report=self.post('api/demo-str?variant=sq-check',{})['report']
        state=self.post('api/project',{'action':'new','name':'SQ review'})
        state=self.post('api/project',{'action':'save','document':state['document'],'report':report})
        identifier=state['selected_analysis_id'];machine=state['effective']['machine_result']
        state=self.post('api/project',{'action':'review','document':state['document'],'analysis_id':identifier,
            'review_action':'change-label','target_kind':'call','target_id':report['calls'][0]['id'],
            'value':'Own reviewed label','actor':'Synthetic reviewer','reason':'Own software check'})
        self.assertEqual(state['effective']['machine_result'],machine)
        self.assertEqual(state['effective']['review_quality']['sample_summary']['freshness'],'stale')
        state=self.post('api/project',{'action':'undo','document':state['document'],'analysis_id':identifier,
            'event_id':state['events'][-1]['event_id'],'actor':'Synthetic reviewer','reason':'Undo check'})
        self.assertEqual(state['effective']['machine_result'],machine)
        self.assertEqual(state['effective']['review_quality']['sample_summary']['freshness'],'unchanged')

    def test_direct_ladder_calls_keep_expected_labels_and_confidence_through_qc(self):
        current=self.post('api/demo-str?variant=ladder-calls',{})
        legacy=self.post('api/demo-str?variant=ladder-sample-filters',{})
        a,b=current['report'],legacy['report']
        self.assertEqual(a['source_sha256'],b['source_sha256']);self.assertEqual(a['ladder_identity'],b['ladder_identity'])
        self.assertEqual(a['sizing'],b['sizing'])
        self.assertEqual([c['label'] for c in a['calls']],['B','C']);self.assertEqual(b['calls'],[])
        observed=a['quality']['ladder_calling']['markers'][0]['calls']
        self.assertEqual([c['label'] for c in observed],list('ABCD'))
        confidence=a['ladder']['markers']['ExampleM']['confidence']
        self.assertLess(confidence,1)
        self.assertTrue(all(c['confidence']==confidence for c in observed))
        self.assertEqual(a['quality']['ladder_gq']['markers'][0]['call_scores'][0]['initial_confidence'],confidence)
        state=self.post('api/project',{'action':'new','name':'Ladder calls HTTP'})
        state=self.post('api/project',{'action':'save','document':state['document'],'report':a})
        imported=self.post('api/project',{'action':'import','document':state['document']})
        self.assertEqual(imported['effective']['machine_result']['quality'],a['quality'])

    def test_run_route_keeps_failed_ladder_evidence_and_project_aggregation(self):
        demo=self.post('api/demo-str-run',{})
        report=self.post('api/str-run',{'sample':demo['sample'],'ladders':demo['ladders'],'kit':demo['kit'],
            'ladder_quality_definition':demo['ladder_quality_definition']})
        self.assertEqual(report,demo['report'])
        self.assertEqual([c['label'] for c in report['calls']],list('BC'))
        self.assertEqual([s['quality_code'] for s in report['quality']['run_ladders']],[0,0,2])
        key=report['ladder']['markers']['ExampleM']['per_key']['bins']['A']
        self.assertEqual(len(key['contributors']),2);self.assertEqual(len(key['excluded']),1)
        state=self.post('api/project',{'action':'new','name':'Run HTTP'})
        state=self.post('api/project',{'action':'save','document':state['document'],'report':report})
        imported=self.post('api/project',{'action':'import','document':state['document']})
        self.assertEqual(imported['effective']['machine_result']['ladder'],report['ladder'])
        self.assertEqual(imported['effective']['machine_result']['quality'],report['quality'])

    def test_persistent_run_revisions_recalculate_siblings_and_keep_review_history(self):
        demo=self.post('api/demo-str-run',{})
        inputs={'schema_version':1,'kit':demo['kit'],'ladder_quality_definition':demo['ladder_quality_definition'],
            'missing_offset_policy':'reject','ladders':demo['ladders'],
            'samples':[{'sample_key':key,'analysis':demo['sample']} for key in ('First','Second')]}
        state=self.post('api/project',{'action':'new','name':'Persistent HTTP'})
        state=self.post('api/project',{'action':'run-create','document':state['document'],'name':'Own Run',
            'inputs':inputs,'actor':'Tester','reason':'Create run'})
        run=state['selected_run_id'];root=state['selected_run_revision'];old=root['outputs'][0]['analysis_id']
        self.assertEqual(len(state['analyses']),2)
        state=self.post('api/project',{'action':'review','document':state['document'],'analysis_id':old,
            'review_action':'exclude','target_kind':'call','target_id':state['effective']['machine_result']['calls'][0]['id'],
            'actor':'Reviewer','reason':'Own review'})
        state=self.post('api/project',{'action':'run-replace-ladder','document':state['document'],'run_id':run,
            'parent_revision_id':root['revision_id'],'ladder_id':'Own-A.fsa','analysis':demo['ladders'][2]['analysis'],
            'actor':'Tester','reason':'Replace ladder'})
        self.assertEqual(len(state['analyses']),4);self.assertEqual(len(state['run_revisions']),2)
        self.assertEqual(state['effective']['machine_result']['ladder']['markers']['ExampleM']['bin_offsets']['A'],-.5)
        self.assertEqual(state['events'],[])
        state=self.post('api/project',{'action':'select','document':state['document'],'analysis_id':old})
        self.assertEqual(state['effective']['run_contexts'][0]['freshness'],'superseded');self.assertEqual(len(state['events']),1)
        imported=self.post('api/project',{'action':'import','document':state['document']})
        self.assertEqual(len(imported['run_revisions']),2)
        self.post('api/project',{'action':'run-revise','document':state['document'],'run_id':run,
            'parent_revision_id':root['revision_id'],'inputs':inputs,'actor':'T','reason':'Stale parent'},expected=400)

    def test_compact_spike_observations_and_project_survive_http_roundtrip(self):
        demo=self.post('api/demo-str?variant=spike',{})
        report=self.post('api/str',{'sample':demo['sample'],'ladder':demo['ladder'],'kit':demo['kit']})
        self.assertEqual(report['calls'],demo['report']['calls'])
        self.assertEqual(report['peaks'],demo['report']['peaks'])
        state=self.post('api/project',{'action':'new','name':'Spike HTTP'})
        state=self.post('api/project',{'action':'save','document':state['document'],'report':report})
        self.assertEqual(state['effective']['machine_result']['peaks'],report['peaks'])


if __name__=='__main__':unittest.main()
