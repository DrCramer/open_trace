from copy import deepcopy
import json
import unittest
from genemapper_open.allele_calling import call_sample
from genemapper_open.spike import load_spike_definition
from genemapper_open.spike_adjustment import adjust_ladder_spikes,apply_peak_states,marker_has_spike
from genemapper_open.str_demo import make_str_demo,make_spike_adjustment_demo,make_quality_demo_definition,make_negative_demo_definitions,make_ladder_quality_demo_definition
from genemapper_open.str_workflow import run_str_workflow,analyze_str_files,compact_analysis
from genemapper_open.server import analyze
from genemapper_open.marker_quality_workflow import evaluate_marker_quality
from genemapper_open.control_workflow import attach_control_result
from genemapper_open.ladder_quality_workflow import gate_ladder_offsets
from genemapper_open.project_api import project_action


POLICY={'sample_cutoff':True,'sample_marker_envelope':True,'ladder_height_ratio':.2}


def peak(bp,height=1000,spike=False,channel='DATA1',**extra):
    return {'channel':channel,'scan':bp*20,'size_bp':bp,'height':height,'spike':spike,'offscaled':False,**extra}


def inputs(*markers):
    prototype=make_str_demo()['kit']['markers'][0]
    rows=[]
    for changes in markers or ({},):
        row=deepcopy(prototype);row.update(changes);row['parameters']['cutoff_ratio']=changes.get('ratio',.2)
        row.pop('ratio',None)
        row['bins']=[b for b in row['bins'] if row['min_bp']<=b['center_bp']<=row['max_bp']]
        row['ladder_labels']=[b['label'] for b in row['bins']]
        rows.append(row)
    kit={'schema_version':1,'name':'Own adjustment boundary fixture','markers':rows}
    offsets={'status':'accepted','markers':{m['name']:{'status':'accepted','bin_offsets':{b['label']:0 for b in m['bins']},
        'min_offset_bp':0,'max_offset_bp':0} for m in rows}}
    return kit,offsets


class AdjustmentBoundaryTests(unittest.TestCase):
    def run_sample(self,peaks,*,markers=(),policy=None):
        kit,offsets=inputs(*markers)
        return call_sample(peaks,kit,offsets,sizing_accepted=True,spike_adjustments=POLICY if policy is None else policy)

    def test_equal_cutoff_clears_but_does_not_recall_excluded_peak(self):
        report=self.run_sample([peak(168,200,True),peak(176),peak(184)])
        marker=report['markers'][0]
        self.assertEqual([c['label'] for c in marker['calls']],['B','C'])
        self.assertEqual(marker['cutoff_height'],200)
        self.assertEqual(marker['peak_review'][0]['state'],'excluded_artifact')
        self.assertIn('spike',marker['peak_review'][0]['flags'])
        self.assertEqual(marker['spike_state']['metric'],0)
        self.assertEqual(report['spike_adjustments']['cleared_spikes'],1)
        self.assertFalse(report['spike_adjustments']['calling_repeated'])

    def test_above_cutoff_remains_current(self):
        report=self.run_sample([peak(168,200.001,True),peak(176)])
        self.assertTrue(marker_has_spike(report['markers'][0]))
        self.assertEqual(report['spike_adjustments']['current_spikes'],1)

    def test_zero_cutoff_has_no_clearing_branch(self):
        report=self.run_sample([peak(168,1,True),peak(176)],markers=({'ratio':0},))
        self.assertEqual(report['spike_adjustments']['current_spikes'],1)

    def test_no_unflagged_candidate_does_not_invent_cutoff(self):
        report=self.run_sample([peak(168,1,True),peak(176,1000,pullup=True)])
        marker=report['markers'][0]
        self.assertNotIn('cutoff_height',marker)
        self.assertEqual(marker['spike_state']['candidate_count'],0)
        self.assertTrue(marker_has_spike(marker))

    def test_disabled_sample_height_rule_keeps_spike(self):
        report=self.run_sample([peak(168,1,True),peak(176)],policy=POLICY|{'sample_cutoff':False})
        self.assertEqual(report['spike_adjustments']['current_spikes'],1)

    def test_later_overlapping_marker_consumes_updated_flags_once(self):
        report=self.run_sample([peak(168,200,True),peak(176)],
            markers=({'name':'First'},{'name':'Second','ratio':.1}))
        first,second=report['markers']
        self.assertEqual([c['label'] for c in first['calls']],['B'])
        self.assertEqual([c['label'] for c in second['calls']],['A','B'])
        self.assertEqual(first['peak_review'][0]['state'],'excluded_artifact')
        self.assertNotIn('spike',second['peak_review'][0]['flags'])

    def test_earlier_metric_is_not_rewritten_after_later_marker(self):
        report=self.run_sample([peak(168,150,True),peak(176)],
            markers=({'name':'First','ratio':.1},{'name':'Second','ratio':.2}))
        self.assertEqual([m['spike_state']['metric'] for m in report['markers']],[1,0])
        self.assertEqual(report['spike_adjustments']['current_spikes'],0)

    def test_envelope_keeps_inclusive_edges_and_internal_gap(self):
        peaks=[peak(bp,1,True) for bp in (159,160,170,180,190,200,201)]
        report=self.run_sample(peaks,markers=({'name':'Low','min_bp':160,'max_bp':170},
            {'name':'High','min_bp':190,'max_bp':200}))
        states=report['spike_adjustments']['peak_states']
        self.assertEqual([s['current_spike'] for s in states],[False,True,True,True,True,True,False])
        self.assertEqual(report['spike_adjustments']['channel_envelopes'],{'DATA1':[160,200]})
        self.assertEqual([m['spike_state']['metric'] for m in report['markers']],[1,1])

    def test_unprocessed_channel_and_disabled_envelope_are_unchanged(self):
        report=self.run_sample([peak(150,1,True),peak(150,1,True,channel='DATA2')],
            policy=POLICY|{'sample_marker_envelope':False})
        self.assertEqual(report['spike_adjustments']['current_spikes'],2)
        report=self.run_sample([peak(150,1,True),peak(150,1,True,channel='DATA2')])
        self.assertEqual(report['spike_adjustments']['current_spikes'],1)

    def test_rejected_marker_does_not_supply_envelope(self):
        kit,offsets=inputs();offsets['status']='rejected'
        report=call_sample([peak(150,1,True)],kit,offsets,sizing_accepted=True,spike_adjustments=POLICY)
        self.assertEqual(report['status'],'rejected')
        self.assertEqual(report['spike_adjustments']['channel_envelopes'],{})
        self.assertEqual(report['spike_adjustments']['current_spikes'],1)

    def test_ladder_max_is_across_processed_markers_of_same_channel(self):
        peaks=[peak(168,150,True),peak(169,300),peak(192),peak(193,500,True),
               peak(176,10000,channel='DATA2'),peak(180,1500,True,channel='DATA2')]
        kit,offsets=inputs({'name':'Low','min_bp':160,'max_bp':175},
            {'name':'High','min_bp':185,'max_bp':200},{'name':'Other','channel':'DATA2'})
        before=call_sample(peaks,kit,offsets,sizing_accepted=True)
        after=adjust_ladder_spikes(peaks,before,POLICY)
        states=after['spike_adjustments']['peak_states']
        self.assertFalse(states[0]['current_spike'])
        self.assertTrue(states[3]['current_spike'])
        self.assertFalse(states[5]['current_spike'])
        self.assertEqual(after['spike_adjustments']['channel_thresholds']['DATA1']['cutoff_height'],200)
        self.assertEqual([m['spike_state']['metric'] for m in after['markers']],[0,1,0])
        self.assertEqual([m['calls'] for m in after['markers']],[m['calls'] for m in before['markers']])

    def test_ladder_clears_unlinked_spike_of_same_channel(self):
        peaks=[peak(150,200,True),peak(176)]
        kit,offsets=inputs();calls=call_sample(peaks,kit,offsets,sizing_accepted=True)
        after=adjust_ladder_spikes(peaks,calls,POLICY)
        self.assertEqual(after['spike_adjustments']['current_spikes'],0)
        self.assertEqual(after['markers'][0]['spike_state']['entry_spike_indices'],[])

    def test_ladder_zero_ratio_and_empty_called_list_do_not_clear(self):
        for peaks,policy in (([peak(168,1,True)],POLICY),
                             ([peak(168,1,True),peak(176)],POLICY|{'ladder_height_ratio':0})):
            kit,offsets=inputs();calls=call_sample(peaks,kit,offsets,sizing_accepted=True)
            after=adjust_ladder_spikes(peaks,calls,policy)
            self.assertEqual(after['spike_adjustments']['current_spikes'],1)
            self.assertIsNone(after['spike_adjustments']['channel_thresholds']['DATA1']['cutoff_height'])

    def test_history_and_inputs_are_preserved(self):
        peaks=[peak(168,200,True,flags={'spike':True,'pullup':False},spike_classification={'current_spike':True}),peak(176)]
        before=deepcopy(peaks);report=self.run_sample(peaks)
        adjusted=apply_peak_states(peaks,report['spike_adjustments'])
        self.assertEqual(peaks,before)
        self.assertTrue(adjusted[0]['spike_classification']['current_spike'])
        self.assertFalse(adjusted[0]['spike']);self.assertFalse(adjusted[0]['flags']['spike'])
        self.assertTrue(adjusted[0]['spike_adjustment']['initial_spike'])
        json.dumps(adjusted,allow_nan=False)

    def test_schema2_requires_complete_finite_explicit_policy(self):
        valid=make_spike_adjustment_demo()['settings']['spike_definition']
        self.assertEqual(load_spike_definition(valid),valid)
        for changes in ({'sample_cutoff':1},{'sample_marker_envelope':None},
                        {'ladder_height_ratio':-1},{'ladder_height_ratio':float('inf')},{'extra':True}):
            invalid=deepcopy(valid);invalid['adjustments'].update(changes)
            with self.assertRaises(ValueError):load_spike_definition(invalid)
        for version in (1,True,3):
            invalid=deepcopy(valid);invalid['schema_version']=version
            with self.assertRaises(ValueError):load_spike_definition(invalid)
        del valid['adjustments']['sample_cutoff']
        with self.assertRaises(ValueError):load_spike_definition(valid)

    def test_invalid_metric_is_not_silently_treated_as_no_spike(self):
        for metric in (None,False,-2,2,'0'):
            with self.assertRaises(ValueError):marker_has_spike({'spike_state':{'metric':metric}})


class AdjustmentWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture=make_spike_adjustment_demo();x=cls.fixture
        cls.sample=analyze(x['sample_bytes'],'own.fsa',**x['settings'])
        cls.ladder=analyze(x['ladder_bytes'],'ladder.fsa',**x['settings'])
        cls.report=run_str_workflow(cls.sample,cls.ladder,x['kit'])

    def test_real_pipeline_low_spike_is_cleared_without_a_call(self):
        report=self.report
        self.assertEqual([c['label'] for c in report['calls']],['B','C'])
        self.assertTrue(report['peaks'][0]['spike_classification']['current_spike'])
        self.assertFalse(report['peaks'][0]['spike'])
        self.assertTrue(report['provenance']['downstream_spike_adjustments_applied'])
        self.assertEqual(report['quality']['spike_adjustments']['sample']['cleared_spikes'],1)
        self.assertEqual(evaluate_marker_quality(report,make_quality_demo_definition())['markers'][0]['score']['gq'],1)

    def test_initial_only_definition_keeps_old_metric_on_identical_bytes(self):
        x=make_spike_adjustment_demo(adjusted=False)
        old=analyze_str_files(x['sample_bytes'],x['ladder_bytes'],x['kit'],settings=x['settings'])
        self.assertEqual(old['source_sha256'],self.report['source_sha256'])
        self.assertEqual(old['sizing'],self.report['sizing'])
        self.assertEqual(old['calls'],self.report['calls'])
        self.assertTrue(old['peaks'][0]['spike'])
        self.assertEqual(evaluate_marker_quality(old,make_quality_demo_definition())['markers'][0]['score']['gq'],.5)

    def test_explicit_quality_flag_still_applies(self):
        definition=make_quality_demo_definition();definition['markers']['ExampleM']['flags']['spike']=True
        self.assertEqual(evaluate_marker_quality(self.report,definition)['markers'][0]['score']['gq'],.5)

    def test_negative_candidate_count_retains_initial_exclusion(self):
        definition,control=make_negative_demo_definitions()
        control['markers']['ExampleM'].update(quality_sensor=True,expected_alleles=['B','C'])
        report=attach_control_result(self.report,control)
        zero=report['quality']['controls']['markers'][0]['zero_peak']
        self.assertEqual(zero['candidate_count'],2)
        self.assertEqual(zero['value'],1)
        quality=evaluate_marker_quality(report,definition)
        self.assertEqual(quality['markers'][0]['score']['metrics']['spike'],0)
        self.assertEqual(quality['markers'][0]['score']['gq'],1)

    def test_ladder_quality_receives_adjusted_metric_without_recalling(self):
        x=self.fixture;ladder=deepcopy(self.ladder)
        # An own scalar extra ladder peak is initially excluded; matching still
        # has its four regular peaks. The later clear must not create an OL call.
        row=next(c for c in ladder['channels'] if c['tag']=='DATA1')
        extra=deepcopy(row['peaks'][0]);extra.update(scan=extra['scan']+40,size_bp=170.3,height=150,spike=True)
        extra['spike_classification']['current_spike']=True
        row['peaks'].append(extra)
        report=run_str_workflow(self.sample,ladder,x['kit'],ladder_quality_definition=make_ladder_quality_demo_definition())
        self.assertEqual(report['status'],'tentative')
        self.assertTrue(report['quality']['ladder_gq']['gate_passed'])
        self.assertFalse(report['quality']['ladder_gq']['markers'][0]['observed_flags']['spike'])
        self.assertEqual(report['quality']['spike_adjustments']['ladder']['cleared_spikes'],1)
        observations=report['quality']['spike_adjustments']['ladder_call_observations']
        self.assertEqual([c['label'] for c in observations['markers'][0]['calls']],list('ABCD'))
        self.assertEqual(observations['markers'][0]['peak_review'][1]['state'],'excluded_artifact')

    def test_project_roundtrip_keeps_initial_history_and_current_metric(self):
        created=project_action({'action':'new','name':'Own adjusted spike project'})
        saved=project_action({'action':'save','document':created['document'],'report':self.report})
        restored=project_action({'action':'import','document':saved['document']})
        machine=restored['effective']['machine_result']
        self.assertEqual(machine['peaks'],self.report['peaks'])
        self.assertEqual(machine['allele_calling'],self.report['allele_calling'])
        self.assertEqual(machine['quality'],self.report['quality'])

    def test_invalid_sample_flags_reject_without_fabricating_adjustment(self):
        sample=deepcopy(self.sample);sample['channels'][0]['peaks'][0]['flags']={'spike':'yes'}
        report=run_str_workflow(sample,self.ladder,self.fixture['kit'])
        self.assertEqual(report['status'],'rejected');self.assertEqual(report['calls'],[])
        self.assertEqual(report['quality']['spike_adjustments']['sample']['status'],'unavailable')

    def test_failed_ladder_quality_still_withholds_sample_calls(self):
        report=run_str_workflow(self.sample,self.ladder,self.fixture['kit'],
            ladder_quality_definition=make_ladder_quality_demo_definition(reject=True))
        self.assertEqual(report['status'],'rejected');self.assertEqual(report['calls'],[])
        self.assertEqual(report['quality']['spike_adjustments']['sample']['channel_envelopes'],{})
