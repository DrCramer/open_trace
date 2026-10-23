from copy import deepcopy
import json
import math
import unittest
from genemapper_open.ladder import match_ladder
from genemapper_open.ladder_calling import ladder_call_observations
from genemapper_open.ladder_quality_workflow import gate_ladder_offsets
from genemapper_open.spike_adjustment import adjust_ladder_spikes,apply_peak_states
from genemapper_open.allele_calling import call_sample
from genemapper_open.str_demo import make_str_demo,make_ladder_quality_demo_definition,make_ladder_calling_demo
from genemapper_open.str_workflow import analyze_str_files


def peak(bp,height=1000,channel='DATA1',**extra):
    return {'channel':channel,'scan':200+20*bp,'size_bp':bp,'height':height,'offscaled':False,**extra}


def fixture(heights=(1000,1000,1000,1000)):
    kit=make_str_demo()['kit']
    peaks=[peak(bp,h,id='own-'+str(i)) for i,(bp,h) in enumerate(zip((168.3,176.3,184.3,192.3),heights))]
    return kit,peaks


def matching(kit,peaks):return match_ladder(peaks,kit,size_standard_max_bp=260,sizing_accepted=True)


class LadderCallTests(unittest.TestCase):
    def test_sample_cutoff_minus_a_stutter_do_not_run_again_on_matched_ladder(self):
        kit,peaks=fixture();kit['markers'][0]['parameters'].update(cutoff_ratio=1,minus_a_ratio=1,
            minus_a_min_bp=7,minus_a_max_bp=9,backward_stutter_ranges=[{'min_bp':7,'max_bp':9,'ratio':1}])
        match=matching(kit,peaks);calls=ladder_call_observations(peaks,kit,match)
        self.assertEqual(calls['status'],'tentative')
        self.assertEqual([c['label'] for c in calls['markers'][0]['calls']],list('ABCD'))
        self.assertEqual(calls['markers'][0]['stutter_calls'],[])
        self.assertEqual(call_sample(peaks,kit,match,sizing_accepted=True)['markers'][0]['calls'],[])

    def test_expected_labels_bypass_ambiguous_sample_bin_reassignment(self):
        kit,peaks=fixture()
        for b in kit['markers'][0]['bins']:b.update(left_bp=30,right_bp=30)
        match=matching(kit,peaks);observed=ladder_call_observations(peaks,kit,match)
        self.assertEqual([c['bin_hits'] for c in observed['markers'][0]['calls']],[[x] for x in 'ABCD'])
        self.assertFalse(any(c['off_ladder'] for c in observed['markers'][0]['calls']))
        legacy=call_sample(peaks,kit,match,sizing_accepted=True)
        self.assertTrue(all(c['off_ladder'] for c in legacy['markers'][0]['calls']))

    def test_all_inclusive_ladder_window_peaks_are_linked_before_filters(self):
        kit,peaks=fixture();peaks.extend([peak(153,100,spike=True),peak(260,10),peak(152.9,10)])
        match=matching(kit,peaks);observed=ladder_call_observations(peaks,kit,match)
        reviews={r['peak_index']:r for r in observed['markers'][0]['peak_review']}
        self.assertIn(4,reviews);self.assertIn(5,reviews);self.assertNotIn(6,reviews)
        self.assertEqual(reviews[4]['state'],'excluded_artifact')
        self.assertEqual(reviews[5]['state'],'below_ladder_cutoff')
        self.assertEqual(observed['unassigned_peak_indices'],[6])

    def test_offscale_outside_sample_window_below_cutoff_still_affects_ladder_qc(self):
        kit,peaks=fixture();peaks.append(peak(154,10,offscaled=True))
        match=matching(kit,peaks)
        _,quality=gate_ladder_offsets(peaks,kit,match,make_ladder_quality_demo_definition(),
            sizing={'status':'accepted','base_quality':{'status':'evaluated','score':1}})
        self.assertTrue(quality['markers'][0]['observed_flags']['offscale'])
        self.assertFalse(quality['gate_passed'])

    def test_ladder_cutoff_equality_survives_and_links_are_not_reduced_by_cap(self):
        kit,peaks=fixture();kit['markers'][0]['parameters']['ladder_cutoff_ratio']=.3
        for bp in (154.1,157.2,161.3,165.4,205.5,220.6):peaks.append(peak(bp,300))
        peaks.append(peak(230.7,299))
        match=matching(kit,peaks);self.assertEqual(match['status'],'accepted')
        observed=ladder_call_observations(peaks,kit,match)
        reviews=observed['markers'][0]['peak_review']
        self.assertEqual(len(reviews),len(peaks))
        self.assertEqual(next(r for r in reviews if r['peak_index']==10)['state'],'below_ladder_cutoff')
        self.assertFalse(any(r['state']=='below_ladder_cutoff' for r in reviews if 4<=r['peak_index']<=9))

    def test_top_2n_rejected_peaks_remain_linked(self):
        kit,peaks=fixture()
        for bp in (154.1,157.2,161.3,165.4,205.5,220.6):peaks.append(peak(bp,300))
        peaks.append(peak(230.7,100))
        match=matching(kit,peaks);self.assertEqual(match['status'],'accepted')
        observed=ladder_call_observations(peaks,kit,match)
        self.assertEqual(next(r for r in observed['markers'][0]['peak_review'] if r['peak_index']==10)['state'],
            'below_top_2n_cutoff')

    def test_unmatched_height_confidence_is_preserved_then_composed_with_gq_sq(self):
        kit,peaks=fixture((700,900,1100,1300));peaks.append(peak(154.2,800))
        match=matching(kit,peaks);marker=match['markers']['ExampleM'];self.assertEqual(marker['status'],'accepted')
        self.assertAlmostEqual(marker['diagnostics']['unmatched_height_threshold'],1000-math.sqrt(200000/3))
        self.assertEqual(marker['diagnostics']['penalty_unmatched_count'],1)
        self.assertAlmostEqual(marker['confidence'],marker['diagnostics']['best_score']*.75)
        observed=ladder_call_observations(peaks,kit,match)
        self.assertTrue(all(c['confidence']==marker['confidence'] for c in observed['markers'][0]['calls']))
        recipe=make_ladder_quality_demo_definition();recipe['ladder_sq']={'source':'explicit','score':.4}
        _,quality=gate_ladder_offsets(peaks,kit,match,recipe,sizing={},call_observations=observed)
        self.assertEqual(quality['markers'][0]['score']['gq'],1)
        self.assertAlmostEqual(quality['markers'][0]['call_scores'][0]['after_sample_sq'],marker['confidence']*.4)

    def test_clear_ranges_remove_calls_after_offsets_without_changing_source_bp(self):
        kit,peaks=fixture();kit['markers'][0]['parameters']['clear_ranges']=[{'min_bp':168.3,'max_bp':168.3}]
        before=deepcopy(peaks);match=matching(kit,peaks);observed=ladder_call_observations(peaks,kit,match)
        self.assertEqual([c['label'] for c in observed['markers'][0]['calls']],list('BCD'))
        self.assertIn('A',match['markers']['ExampleM']['bin_offsets'])
        self.assertEqual(observed['markers'][0]['peak_review'][0]['state'],'clear_range')
        self.assertEqual(peaks,before)

    def test_cleared_calls_do_not_supply_ladder_spike_height_maximum(self):
        kit,peaks=fixture();kit['markers'][0]['parameters']['clear_ranges']=[{'min_bp':160,'max_bp':200}]
        peaks.append(peak(154,100,spike=True));match=matching(kit,peaks)
        observed=ladder_call_observations(peaks,kit,match)
        adjusted=adjust_ladder_spikes(peaks,observed,{'sample_cutoff':True,'sample_marker_envelope':True,'ladder_height_ratio':.2})
        self.assertEqual(adjusted['markers'][0]['calls'],[])
        self.assertIsNone(adjusted['spike_adjustments']['channel_thresholds']['DATA1']['max_called_height'])
        self.assertTrue(adjusted['spike_adjustments']['peak_states'][4]['current_spike'])

    def test_channel_local_ordinals_map_exactly_to_global_input_identity(self):
        kit,rows=fixture();peaks=[]
        for p in reversed(rows):
            peaks.extend([p|{'channel':'DATA2','id':'other-'+p['id']},p])
        match=matching(kit,peaks);observed=ladder_call_observations(peaks,kit,match)
        calls=observed['markers'][0]['calls']
        self.assertEqual([c['peak_index'] for c in calls],[7,5,3,1])
        self.assertEqual([c['source_peak_id'] for c in calls],['own-0','own-1','own-2','own-3'])

    def test_corrupt_ordinal_is_rejected_without_coordinate_fallback(self):
        kit,peaks=fixture();match=matching(kit,peaks)
        match['markers']['ExampleM']['matched'][0]['candidate_index']=1
        observed=ladder_call_observations(peaks,kit,match)
        self.assertEqual(observed['status'],'rejected');self.assertEqual(observed['markers'][0]['calls'],[])

    def test_shared_source_peak_across_overlapping_markers_keeps_identity(self):
        prototype=make_str_demo()['kit']['markers'][0]
        markers=[]
        for name,low,high,labels,centers in (('First',188,200,['A','B'],[192,200]),
                                           ('Second',200,212,['X','Y'],[200,208])):
            marker=deepcopy(prototype);marker.update(name=name,min_bp=low,max_bp=high,
                bins=[{'label':label,'center_bp':bp,'left_bp':.5,'right_bp':.5} for label,bp in zip(labels,centers)],
                ladder_labels=labels);markers.append(marker)
        kit={'schema_version':1,'name':'Own shared ladder source','markers':markers}
        peaks=[peak(bp) for bp in (191.7,199.7,207.7)]
        match=matching(kit,peaks);self.assertEqual(match['status'],'accepted')
        observed=ladder_call_observations(peaks,kit,match)
        links=[c for marker in observed['markers'] for c in marker['calls'] if c['peak_index']==1]
        self.assertEqual(len(links),2)
        self.assertEqual([c['label'] for c in links],['B','X'])
        self.assertTrue(all('cross_marker_overlap' in c['flags'] for c in links))

    def test_spike_clear_uses_surviving_calls_without_rematching_or_reinsertion(self):
        kit,peaks=fixture((700,900,1100,1300));peaks.append(peak(154,260,spike=True))
        match=matching(kit,peaks);before=deepcopy(match)
        observed=ladder_call_observations(peaks,kit,match)
        adjusted=adjust_ladder_spikes(peaks,observed,{'sample_cutoff':True,'sample_marker_envelope':True,'ladder_height_ratio':.2})
        self.assertFalse(adjusted['spike_adjustments']['peak_states'][4]['current_spike'])
        self.assertEqual(adjusted['markers'][0]['calls'],observed['markers'][0]['calls'])
        self.assertEqual(match,before)
        self.assertEqual(len(adjusted['markers'][0]['calls']),4)

    def test_incomplete_or_inconsistent_matches_do_not_fabricate_calls(self):
        kit,peaks=fixture()
        for mutate in (lambda m:m['markers']['ExampleM']['matched'].pop(),
            lambda m:m['markers']['ExampleM']['matched'][0].update(confidence=.1),
            lambda m:m['markers']['ExampleM']['matched'][0].update(cleared=True)):
            match=matching(kit,peaks);mutate(match)
            observed=ladder_call_observations(peaks,kit,match)
            self.assertEqual(observed['status'],'rejected');self.assertEqual(observed['markers'][0]['calls'],[])

    def test_missing_matching_never_falls_back_to_sample_calling(self):
        kit,peaks=fixture();match=matching(kit,peaks);match['markers']['ExampleM']['status']='rejected'
        observed=ladder_call_observations(peaks,kit,match)
        self.assertEqual(observed['status'],'rejected');self.assertEqual(observed['markers'][0]['calls'],[])

    def test_observation_policy_mismatch_is_rejected(self):
        kit,peaks=fixture();match=matching(kit,peaks);observed=ladder_call_observations(peaks,kit,match)
        recipe=make_ladder_quality_demo_definition();recipe['calling_parameters_source']='kit_sample_parameters'
        with self.assertRaises(ValueError):gate_ladder_offsets(peaks,kit,match,recipe,sizing={},call_observations=observed)

    def test_supplied_stale_observations_cannot_hide_a_new_offscaled_window_peak(self):
        kit,peaks=fixture();match=matching(kit,peaks)
        old=ladder_call_observations(peaks,kit,match)
        peaks.append(peak(154,10,offscaled=True));match=matching(kit,peaks)
        recipe=make_ladder_quality_demo_definition();recipe['ladder_sq']={'source':'explicit','score':1}
        with self.assertRaises(ValueError):
            gate_ladder_offsets(peaks,kit,match,recipe,sizing={},call_observations=old)
        _,fresh=gate_ladder_offsets(peaks,kit,match,recipe,sizing={})
        self.assertFalse(fresh['gate_passed'])
        self.assertTrue(fresh['markers'][0]['observed_flags']['offscale'])

    def test_bound_adjusted_observations_keep_phase_without_reintroducing_cleared_spike(self):
        kit,peaks=fixture();peaks.append(peak(154,100,spike=True))
        match=matching(kit,peaks);original=ladder_call_observations(peaks,kit,match)
        adjusted=adjust_ladder_spikes(peaks,original,
            {'sample_cutoff':True,'sample_marker_envelope':True,'ladder_height_ratio':.2})
        current=apply_peak_states(peaks,adjusted['spike_adjustments'])
        recipe=make_ladder_quality_demo_definition();recipe['ladder_sq']={'source':'explicit','score':1}
        _,quality=gate_ladder_offsets(current,kit,match,recipe,sizing={},call_observations=adjusted)
        self.assertTrue(quality['gate_passed'])
        self.assertFalse(quality['markers'][0]['observed_flags']['spike'])
        self.assertEqual([c['name'] for c in quality['markers'][0]['calls']],list('ABCD'))
        self.assertEqual(adjusted['markers'][0]['peak_review'][0]['state'],'excluded_artifact')

    def test_window_bound_is_rejected_without_silent_peak_truncation(self):
        kit,peaks=fixture()
        peaks.extend(peak(154+i*.001,1) for i in range(510))
        match=matching(kit,peaks);self.assertEqual(match['status'],'accepted')
        observed=ladder_call_observations(peaks,kit,match)
        self.assertEqual(observed['status'],'rejected');self.assertEqual(observed['markers'][0]['peak_review'],[])
        self.assertIn('512',observed['markers'][0]['reasons'][0])

    def test_inputs_and_initial_confidence_remain_unchanged(self):
        kit,peaks=fixture();match=matching(kit,peaks);before=deepcopy((kit,peaks,match))
        observed=ladder_call_observations(peaks,kit,match)
        self.assertEqual((kit,peaks,match),before);json.dumps(observed,allow_nan=False)


class LadderCallingWorkflowTests(unittest.TestCase):
    def test_same_fsa_direct_ladder_calls_pass_while_legacy_sample_filters_fail(self):
        x,recipe=make_ladder_calling_demo()
        current=analyze_str_files(x['sample_bytes'],x['ladder_bytes'],x['kit'],settings=x['settings'],ladder_quality_definition=recipe)
        recipe['calling_parameters_source']='kit_sample_parameters'
        legacy=analyze_str_files(x['sample_bytes'],x['ladder_bytes'],x['kit'],settings=x['settings'],ladder_quality_definition=recipe)
        self.assertEqual(current['source_sha256'],legacy['source_sha256'])
        self.assertEqual(current['ladder_identity'],legacy['ladder_identity'])
        self.assertEqual([c['label'] for c in current['calls']],list('BC'))
        self.assertEqual(legacy['calls'],[])
        self.assertEqual([c['label'] for c in current['quality']['ladder_calling']['markers'][0]['calls']],list('ABCD'))
        self.assertTrue(current['quality']['ladder_gq']['gate_passed'])
        self.assertFalse(legacy['quality']['ladder_gq']['gate_passed'])
        self.assertFalse(current['provenance']['ladder_sample_filters_applied'])
        self.assertTrue(legacy['provenance']['ladder_sample_filters_applied'])
