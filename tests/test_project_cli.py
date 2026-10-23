from contextlib import redirect_stdout,redirect_stderr
import io
import json
from pathlib import Path
import tempfile
import unittest
from genemapper_open.__main__ import main
from genemapper_open.project_store import ProjectStore


class ProjectCliTests(unittest.TestCase):
    def command(self,*args,expected=0,json_result=True):
        out,err=io.StringIO(),io.StringIO()
        with redirect_stdout(out),redirect_stderr(err):code=main(list(map(str,args)))
        self.assertEqual(code,expected,err.getvalue())
        return json.loads(out.getvalue()) if json_result and code==0 else out.getvalue()

    def test_generated_demo_cli_str_project_review_export_import(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);fixture=root/'fixture';db=root/'project.sqlite'
            self.command('str-demo',fixture,json_result=False)
            report=self.command('str',fixture/'sample.fsa','--ladder',fixture/'ladder.fsa',
                '--kit',fixture/'kit.json','--settings',fixture/'settings.json')
            self.assertEqual([c['label'] for c in report['calls']],['B','C'])
            self.command('project','create',db,'--name','CLI synthetic')
            saved=self.command('project','save',db,fixture/'report.json')
            identifier=saved['analysis_id'];target=report['calls'][0]['id']
            changed=self.command('project','review',db,identifier,'change-label','--target-kind','call',
                '--target-id',target,'--value','Reviewed','--actor','Test','--reason','Known synthetic')
            shown=self.command('project','show',db,'--analysis-id',identifier)
            self.assertEqual(shown['analysis']['result']['calls'][0]['label'],'B')
            self.assertEqual(next(t['label'] for t in shown['effective']['review_targets'] if t['target_id']==target),'Reviewed')
            self.command('project','undo',db,identifier,changed['event_id'],'--actor','Test','--reason','Restore')
            output=root/'project.json';self.command('project','export',db,output)
            newdb=root/'imported.sqlite';self.command('project','import',output,newdb)
            with ProjectStore.open(db) as a,ProjectStore.open(newdb) as b:
                self.assertEqual(a.export_json(),b.export_json())

    def test_existing_destinations_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);db=root/'project.sqlite';output=root/'project.json'
            self.command('project','create',db,'--name','Existing')
            self.command('project','create',db,'--name','Overwrite',expected=2)
            self.command('project','export',db,output)
            original=output.read_bytes()
            self.command('project','export',db,output,expected=2)
            self.command('project','import',output,db,expected=2)
            self.assertEqual(output.read_bytes(),original)

    def test_invalid_report_leaves_no_registered_sample(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);db=root/'project.sqlite';report=root/'bad.json'
            self.command('project','create',db,'--name','Validation')
            report.write_text(json.dumps({'source_sha256':'a'*64,'analysis_version':'test',
                'settings':{},'status':'tentative','calls':[{'id':'duplicate'},{'id':'duplicate'}]}))
            self.command('project','save',db,report,expected=2)
            self.assertEqual(self.command('project','list',db)['samples'],[])

    def test_raw_channel_arrays_are_not_in_compact_project(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);db=root/'project.sqlite';report=root/'report.json'
            self.command('project','create',db,'--name','Compact')
            report.write_text(json.dumps({'source_sha256':'a'*64,'analysis_version':'test',
                'peak_settings':{},'sizing':{'status':'rejected'},'channels':[{'values':[123]}]}))
            self.command('project','save',db,report)
            with ProjectStore.open(db) as store:self.assertNotIn('channels',store.list_analyses()[0]['result'])

    def test_control_cli_attaches_digest_and_project_retains_observations(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);fixture=root/'fixture';definition=root/'control.json';output=root/'controlled.json'
            self.command('str-demo',fixture,json_result=False)
            definition.write_text(json.dumps({'schema_version':1,'sample_type':'Positive Control','markers':{
                'ExampleM':{'expected_alleles':['A','B'],'quality_sensor':False,'is_y_marker':False}}}))
            self.command('controls',fixture/'report.json','--definition',definition,'--output',output,json_result=False)
            report=json.loads(output.read_text())
            self.assertEqual(report['quality']['controls']['composite_control']['code'],2)
            self.assertEqual(report['status'],'tentative')
            db=root/'project.sqlite';self.command('project','create',db,'--name','Control evidence')
            saved=self.command('project','save',db,output)
            shown=self.command('project','show',db,'--analysis-id',saved['analysis_id'])
            self.assertEqual(shown['analysis']['result']['quality'],report['quality'])

    def test_marker_quality_cli_exports_measured_subset_with_no_call_rewrite(self):
        from genemapper_open.str_demo import make_quality_demo_definition
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);fixture=root/'fixture';definition=root/'gq.json'
            self.command('str-demo',fixture,json_result=False)
            definition.write_text(json.dumps(make_quality_demo_definition()))
            report=self.command('marker-quality',fixture/'report.json','--definition',definition)
            self.assertEqual(report['quality']['marker_gq']['sample_category'],0)
            self.assertEqual([call['label'] for call in report['calls']],['B','C'])
            self.assertTrue(report['quality']['marker_gq']['all_markers_evaluated'])

    def test_negative_and_amel_schema2_cli_annotations(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);source=root/'report.json';definition=root/'quality.json';control=root/'control.json'
            for variant in ('negative','amel'):
                fixture=root/variant
                self.command('str-demo',fixture,'--variant',variant,json_result=False)
                recipe=json.loads((fixture/'marker-quality.json').read_text(encoding='utf-8'))
                report=json.loads((fixture/'report.json').read_text(encoding='utf-8'))
                source.write_text(json.dumps(report),encoding='utf-8')
                if variant=='negative':
                    controls=json.loads((fixture/'control.json').read_text(encoding='utf-8'))
                    control.write_text(json.dumps(controls),encoding='utf-8')
                    report=self.command('controls',source,'--definition',control)
                    source.write_text(json.dumps(report),encoding='utf-8')
                definition.write_text(json.dumps(recipe),encoding='utf-8')
                output=self.command('marker-quality',source,'--definition',definition)
                quality=output['quality']['marker_gq']
                self.assertTrue(quality['complete_for_requested_subset'])
                self.assertEqual(output['calls'],report['calls'])
                if variant=='negative':self.assertEqual(quality['markers'][0]['score']['gq'],0)
                else:self.assertTrue(quality['amel_cross_check']['mismatch'])

    def test_ladder_quality_cli_examples_and_explicit_str_option(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            for variant,expected in (('ladder',0),('ladder-rejected',3)):
                fixture=root/variant
                self.command('str-demo',fixture,'--variant',variant,expected=expected,json_result=False)
                report=json.loads((fixture/'report.json').read_text(encoding='utf-8'))
                self.assertEqual(report['quality']['ladder_gq']['gate_passed'],expected==0)
                output=root/(variant+'-report.json')
                self.command('str',fixture/'sample.fsa','--ladder',fixture/'ladder.fsa',
                    '--kit',fixture/'kit.json','--settings',fixture/'settings.json',
                    '--ladder-quality',fixture/'ladder-quality.json','--output',output,
                    expected=expected,json_result=False)
                self.assertEqual(json.loads(output.read_text(encoding='utf-8')),report)

    def test_spike_cli_generated_settings_and_direct_size_option(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);fixture=root/'spike'
            self.command('str-demo',fixture,'--variant','spike',json_result=False)
            report=json.loads((fixture/'report.json').read_text(encoding='utf-8'))
            self.assertEqual([c['label'] for c in report['calls']],['B','C'])
            self.assertTrue(any(p.get('spike') for p in report['peaks']))
            direct=self.command('str',fixture/'sample.fsa','--ladder',fixture/'ladder.fsa',
                '--kit',fixture/'kit.json','--settings',fixture/'settings.json')
            self.assertEqual(direct['calls'],report['calls']);self.assertEqual(direct['peaks'],report['peaks'])
            sizing=self.command('size',fixture/'sample.fsa','--analysis-engine','morphology_ratio',
                '--standard-channel','DATA106','--standard-bp','40,70,110,145,195,260',
                '--calibration-model','local_rational','--spike-definition',fixture/'spike.json')
            self.assertEqual(sizing['peak_settings']['spike_definition']['min_height'],100)


    def test_low_spike_cli_preserves_calls_and_adjustment_history(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);reports=[]
            for variant in ('spike-low-initial','spike-low-adjusted'):
                fixture=root/variant
                self.command('str-demo',fixture,'--variant',variant,json_result=False)
                report=json.loads((fixture/'report.json').read_text(encoding='utf-8'));reports.append(report)
                direct=self.command('str',fixture/'sample.fsa','--ladder',fixture/'ladder.fsa',
                    '--kit',fixture/'kit.json','--settings',fixture/'settings.json')
                self.assertEqual(direct['peaks'],report['peaks'])
                self.assertEqual(direct['calls'],report['calls'])
            self.assertEqual(reports[0]['calls'],reports[1]['calls'])
            self.assertEqual(reports[0]['source_sha256'],reports[1]['source_sha256'])
            self.assertEqual(reports[1]['quality']['spike_adjustments']['sample']['cleared_spikes'],1)


    def test_post_sq_demo_cli_reanalysis_and_size_option(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);fixture=root/'sq'
            self.command('str-demo',fixture,'--variant','sq-check',json_result=False)
            report=json.loads((fixture/'report.json').read_text(encoding='utf-8'))
            self.assertEqual(report['sizing']['post_quality']['score'],.5)
            sizing=self.command('size',fixture/'sample.fsa','--analysis-engine','morphology_ratio',
                '--standard-channel','DATA106','--standard-bp','40,70,110,145,195,260',
                '--calibration-model','local_rational','--sizing-quality-definition',fixture/'sizing-quality.json')
            self.assertEqual(sizing['sizing']['post_quality'],report['sizing']['post_quality'])
            replay=self.command('str',fixture/'sample.fsa','--ladder',fixture/'ladder.fsa',
                '--kit',fixture/'kit.json','--settings',fixture/'settings.json')
            self.assertEqual(replay['calls'],report['calls'])
            self.assertEqual(replay['sizing']['post_quality'],report['sizing']['post_quality'])


    def test_ladder_calling_cli_and_legacy_comparison_use_identical_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);reports=[]
            for variant,expected in (('ladder-calls',0),('ladder-sample-filters',3)):
                fixture=root/variant
                self.command('str-demo',fixture,'--variant',variant,expected=expected,json_result=False)
                report=json.loads((fixture/'report.json').read_text(encoding='utf-8'));reports.append(report)
                output=root/(variant+'.json')
                self.command('str',fixture/'sample.fsa','--ladder',fixture/'ladder.fsa','--kit',fixture/'kit.json',
                    '--settings',fixture/'settings.json','--ladder-quality',fixture/'ladder-quality.json',
                    '--output',output,expected=expected,json_result=False)
                annotated=root/(variant+'-annotated.json')
                self.command('marker-quality',output,'--definition',fixture/'marker-quality.json',
                    '--output',annotated,json_result=False)
                self.assertEqual(json.loads(annotated.read_text(encoding='utf-8')),report)
            self.assertEqual(reports[0]['source_sha256'],reports[1]['source_sha256'])
            self.assertEqual(reports[0]['ladder_identity'],reports[1]['ladder_identity'])
            self.assertTrue(reports[0]['quality']['ladder_gq']['gate_passed'])
            self.assertFalse(reports[1]['quality']['ladder_gq']['gate_passed'])

    def test_run_manifest_replays_generated_three_ladder_fixture(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);fixture=root/'run'
            self.command('str-run-demo',fixture,json_result=False)
            report=json.loads((fixture/'report.json').read_text(encoding='utf-8'))
            replay=self.command('str-run',fixture/'run.json')
            self.assertEqual(replay,report)
            self.assertEqual([c['label'] for c in replay['calls']],list('BC'))
            self.assertEqual([s['quality_code'] for s in replay['quality']['run_ladders']],[0,0,2])
            manifest=json.loads((fixture/'run.json').read_text(encoding='utf-8'))
            manifest['ladders']=manifest['ladders'][2:]
            (fixture/'failed-run.json').write_text(json.dumps(manifest),encoding='utf-8')
            failed=json.loads(self.command('str-run',fixture/'failed-run.json',expected=3))
            self.assertEqual(failed['calls'],[])

    def test_persistent_run_cli_revision_export_and_schema_upgrade(self):
        from genemapper_open.str_demo import make_run_demo
        from genemapper_open.server import analyze
        f=make_run_demo();sample=analyze(f['sample_bytes'],'sample.fsa',**f['settings'])
        ladders=[{'ladder_id':r['ladder_id'],'analysis':analyze(r['data'],r['ladder_id'],**f['settings'])} for r in f['ladders']]
        inputs={'schema_version':1,'kit':f['kit'],'ladder_quality_definition':f['ladder_quality_definition'],
            'missing_offset_policy':'reject','ladders':ladders,'samples':[{'sample_key':k,'analysis':sample} for k in ('A','B')]}
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);db=root/'project.sqlite';source=root/'run-inputs.json';replacement=root/'replacement.json'
            source.write_text(json.dumps(inputs),encoding='utf-8');replacement.write_text(json.dumps(ladders[2]['analysis']),encoding='utf-8')
            self.command('project','create',db,'--name','Own Run CLI')
            created=self.command('project','run-create',db,source,'--name','Run','--actor','T','--reason','Create')
            changed=self.command('project','run-replace-ladder',db,created['run_id'],'Own-A.fsa',replacement,
                '--parent-revision',created['revision_id'],'--actor','T','--reason','Replace')
            history=self.command('project','run-show',db,created['run_id'])
            self.assertEqual(len(history['revisions']),2);self.assertEqual(len(changed['outputs']),2)
            export=root/'project.json';self.command('project','export',db,export)
            payload=json.loads(export.read_text(encoding='utf-8'))
            self.assertEqual(payload['schema_version'],2);self.assertEqual(len(payload['analyses']),4)
            legacy={'format':payload['format'],'schema_version':1,'project':dict(payload['project'],schema_version=1),
                'samples':[],'analyses':[],'review_events':[]}
            old_json=root/'v1.json';old_db=root/'v1.sqlite';new_db=root/'upgraded.sqlite'
            old_json.write_text(json.dumps(legacy),encoding='utf-8');self.command('project','import',old_json,old_db)
            before=old_db.read_bytes();upgraded=self.command('project','upgrade',old_db,new_db)
            self.assertEqual(upgraded['project']['schema_version'],2);self.assertEqual(old_db.read_bytes(),before)


if __name__=='__main__':unittest.main()
