from copy import deepcopy
import json
import math
import unittest
from genemapper_open.pullup import load_pullup_definition,apply_pullup_detection,_full_set
from genemapper_open.str_demo import make_pullup_demo_definition


def peak(scan,height=50,**extra):return {'scan':scan,'height':height,'offscaled':False,**extra}
def channel(tag,peaks):return {'tag':tag,'group':'raw','peaks':peaks}


class PullupTests(unittest.TestCase):
    def run_route(self,candidates,references,definition=None,extra=()):
        channels=[channel('DATA1',candidates),channel('DATA2',references),channel('DATA106',[]),*extra]
        return apply_pullup_detection(channels,make_pullup_demo_definition() if definition is None else definition,
            selected_group='raw',reference_channel='DATA106')

    def test_exact_match_and_distance_equality_ratio_strictness(self):
        d=make_pullup_demo_definition();d['detector']['height_ratio_threshold']=.1
        r=self.run_route([peak(10,10),peak(11,9),peak(math.nextafter(11,12),9)],[peak(10,100)],d)
        a,b,c=r['peaks_by_channel']['DATA1']
        self.assertFalse(a['pullup']);self.assertTrue(b['pullup']);self.assertFalse(c['pullup'])
        self.assertEqual(b['pullup_classification']['state']['ratio'],.09)

    def test_right_first_is_not_nearest_or_strongest(self):
        r=self.run_route([peak(10.1)],[peak(10,10000),peak(10.8,1000)])
        c=r['peaks_by_channel']['DATA1'][0]['pullup_classification']
        self.assertEqual(c['matched_reference_id'],'DATA2:1')
        self.assertEqual(len(c['events']),1)

    def test_right_failure_tries_left_second(self):
        r=self.run_route([peak(10.1)],[peak(10,1000),peak(10.8,100)])
        c=r['peaks_by_channel']['DATA1'][0]['pullup_classification']
        self.assertEqual([e['reference_id'] for e in c['events']],['DATA2:1','DATA2:0'])
        self.assertEqual(c['events'][0]['attempt_ratio'],.5)
        self.assertEqual(c['events'][0]['after']['ratio'],-1)
        self.assertEqual(c['state']['ratio'],.05)

    def test_exact_duplicate_search_does_not_scan_alternatives(self):
        r=self.run_route([peak(10)],[peak(10,1000),peak(10,1),peak(10,1000)])
        p=r['peaks_by_channel']['DATA1'][0]
        self.assertFalse(p['pullup'])
        self.assertEqual(p['pullup_classification']['events'][0]['reference_id'],'DATA2:1')
        self.assertEqual(len(p['pullup_classification']['events']),1)
        self.assertEqual(r['summary']['passes'][0]['duplicate_reference_scans'],2)

    def test_sorting_is_explicit_stable_policy_and_retains_original_ids(self):
        r=self.run_route([peak(10)],[peak(11,1000),peak(9,100)])
        c=r['peaks_by_channel']['DATA1'][0]['pullup_classification']
        self.assertEqual(c['matched_reference_id'],'DATA2:0')

    def test_candidate_guards_reference_flags_are_not_filtered(self):
        r=self.run_route([peak(10,offscaled=True),peak(10,pullup=True),peak(10,spike=True),peak(10)],
            [peak(10,1000,offscaled=True,spike=True,pullup=True)])
        c=r['peaks_by_channel']['DATA1']
        self.assertEqual([p['pullup'] for p in c],[False,True,False,True])
        self.assertTrue(all(not p['pullup_classification']['events'] for p in c[:3]))
        self.assertEqual(c[3]['pullup_classification']['state']['artifact_label'],'SPU')

    def test_empty_reference_does_not_run_full_setter(self):
        r=self.run_route([peak(10,artifact_label='custom')],[])
        state=r['peaks_by_channel']['DATA1'][0]['pullup_classification']['state']
        self.assertEqual(state['artifact_label'],'custom');self.assertIsNone(state['peak_called_state'])

    def test_failed_pair_still_clears_label_and_retains_stale_ratio(self):
        r=self.run_route([peak(10,artifact_label='custom',pullup_ratio=.3,pullup_ratio_threshold=.4)],[peak(10,1)])
        state=r['peaks_by_channel']['DATA1'][0]['pullup_classification']['state']
        self.assertFalse(state['pullup']);self.assertEqual(state['artifact_label'],'')
        self.assertEqual(state['peak_called_state'],1);self.assertTrue(state['originally_artifact'])
        self.assertEqual((state['ratio'],state['ratio_threshold']),(.3,.4))

    def test_zero_reference_height_uses_null_attempt_ratio_but_preserves_state_reset(self):
        r=self.run_route([peak(10,artifact_label='A')],[peak(10,0)])
        event=r['peaks_by_channel']['DATA1'][0]['pullup_classification']['events'][0]
        self.assertIsNone(event['attempt_ratio']);self.assertFalse(event['ratio_available'])
        self.assertEqual(event['reason'],'reference_height_not_positive')
        self.assertEqual(event['after']['artifact_label'],'')
        json.dumps(r,allow_nan=False)

    def test_full_true_uses_java_trim_and_false_clears_any_label(self):
        for initial,expected in ((None,'SPU'),('\t ','SPU'),('A','A'),('\u00a0','\u00a0')):
            state={'artifact_label':initial,'ratio':.3,'ratio_threshold':.4}
            _full_set(state,True);self.assertEqual(state['artifact_label'],expected)
            _full_set(state,False);self.assertEqual(state['artifact_label'],'')
            self.assertEqual(state['ratio'],.3)

    def test_ordered_passes_skip_existing_match(self):
        d=make_pullup_demo_definition()
        d['detector']['channel_pairs'].append({'candidate_channel':'DATA1','reference_channel':'DATA3'})
        r=self.run_route([peak(10)],[peak(10,1000)],d,extra=[channel('DATA3',[peak(10,10000)])])
        c=r['peaks_by_channel']['DATA1'][0]['pullup_classification']
        self.assertEqual(c['matched_reference_id'],'DATA2:0')
        self.assertEqual(c['skips'][0]['reason'],'candidate_already_pullup')
        self.assertEqual(len(c['events']),1)

    def test_reference_standard_can_be_read_but_is_never_replaced(self):
        d=make_pullup_demo_definition();d['detector']['channel_pairs'][0]['reference_channel']='DATA106'
        channels=[channel('DATA1',[peak(10)]),channel('DATA106',[peak(10,1000)])]
        before=deepcopy(channels)
        r=apply_pullup_detection(channels,d,selected_group='raw',reference_channel='DATA106')
        self.assertEqual(channels,before);self.assertNotIn('DATA106',r['peaks_by_channel'])
        self.assertTrue(r['peaks_by_channel']['DATA1'][0]['pullup'])

    def test_reference_standard_candidate_and_cross_group_routes_rejected(self):
        d=make_pullup_demo_definition();d['detector']['channel_pairs'][0]['candidate_channel']='DATA106'
        with self.assertRaises(ValueError):self.run_route([],[],d)
        d=make_pullup_demo_definition();d['detector']['channel_pairs'][0]['reference_channel']='DATA3'
        other=channel('DATA3',[]);other['group']='analyzed'
        with self.assertRaises(ValueError):self.run_route([],[],d,extra=[other])

    def test_definitions_unknown_duplicate_nonfinite_and_unbounded_fields_rejected(self):
        with self.assertRaises(ValueError):load_pullup_definition('{"schema_version":1,"schema_version":1}')
        for key,value in (('max_scan_distance',True),('height_ratio_threshold',float('nan')),('phase','unknown')):
            d=make_pullup_demo_definition();d['detector'][key]=value
            with self.assertRaises(ValueError):load_pullup_definition(d)
        d=make_pullup_demo_definition();d['detector']['channel_pairs']*=33
        with self.assertRaises(ValueError):load_pullup_definition(d)

    def test_flags_only_and_unrequested_lists_do_not_infer_detection(self):
        r=self.run_route([peak(10)],[peak(10,1000)],make_pullup_demo_definition(detector=False))
        self.assertEqual(r['peaks_by_channel'],{});self.assertEqual(r['summary']['status'],'not_requested')
        self.assertFalse(r['summary']['original_automatic_route_confirmed'])

    def test_reapplying_existing_history_is_rejected(self):
        r=self.run_route([peak(10)],[peak(10,1000)])
        with self.assertRaises(ValueError):self.run_route(r['peaks_by_channel']['DATA1'],[peak(10,1000)])


if __name__=='__main__':unittest.main()
