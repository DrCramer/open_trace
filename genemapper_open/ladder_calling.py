"""Direct ladder call observations from accepted matching, before QC.

Fresh-state contract: preserve expected label/bin, observed BP, exact source
identity and matcher confidence. Clear ranges remove calls after offsets have
been committed. All window peaks remain linked for artifact QC. No sample
cutoff, minus-A, stutter or sample bin reassignment occurs here.
"""
from collections.abc import Mapping
from .fastcopy import fastcopy
from .allele_calling import _prepare_peaks,MAX_MARKER_PEAKS
from .kit_definition import load_kit_definition
from .quality_control import _number


def ladder_call_observations(peaks,kit,matching):
    result={'status':'rejected','reasons':[],'markers':[],'unassigned_peak_indices':[],
        'research_only':True,'external_validation':False,'original_runtime_validated':False,
        'diagnostics':{'method':'ladder_matching_calls','source_identity':'channel_local_input_ordinal_to_flat_input_index',
            'sample_filters_applied':False,'matching_repeated':False}}
    definition=load_kit_definition(kit)
    prepared=_prepare_peaks(peaks)
    by_index={p['peak_index']:p for p in prepared}
    # match_ladder preserves candidate_index relative to each ORIGINAL channel
    # list, before sorting. Do not infer identity from numeric coordinates.
    channel_indices={}
    for index,peak in enumerate(peaks):channel_indices.setdefault(peak['channel'],[]).append(index)
    if not isinstance(matching,Mapping) or not isinstance(matching.get('markers'),Mapping):
        raise ValueError('Explicit marker ladder matching result required')
    assigned=set()
    for marker in definition['markers']:
        name,channel=marker['name'],marker['channel']
        row={'name':name,'channel':channel,'status':'rejected','reasons':[],
            'calls':[],'stutter_calls':[],'peak_review':[],'events':[]}
        result['markers'].append(row)
        source=matching['markers'].get(name)
        if not isinstance(source,Mapping) or source.get('status')!='accepted':
            row['reasons'].append('Ladder marker matching not accepted');continue
        try:
            if source.get('channel')!=channel:raise ValueError('Matching marker channel differs from kit')
            if matching.get('status') not in ('accepted','partial'):raise ValueError('Overall ladder matching not accepted or partial')
            window=source.get('window')
            if not isinstance(window,Mapping) or window.get('inclusive') is not True:
                raise ValueError('Explicit inclusive ladder matching window required')
            low=_number(window.get('min_bp'),'ladder window min',-1e6,1e6)
            high=_number(window.get('max_bp'),'ladder window max',0,1e6)
            if low>high:raise ValueError('Reversed ladder matching window')
            linked=[p for p in prepared if p['channel']==channel and low<=p['size_bp']<=high]
            if len(linked)>MAX_MARKER_PEAKS:raise ValueError('Ladder window exceeds512 linked peaks; no truncation applied')
            rows=source.get('matched')
            if not isinstance(rows,list) or len(rows)!=len(marker['ladder_labels']):
                raise ValueError('Complete matched expected ladder rows required')
            local=channel_indices.get(channel,[]);selected={};bins={b['label']:b for b in marker['bins']}
            marker_confidence=_number(source.get('confidence'),'marker ladder confidence',0,1e12)
            for index,match in enumerate(rows):
                if not isinstance(match,Mapping):raise ValueError('Matched ladder row required')
                ordinal=match.get('candidate_index')
                if type(ordinal) is not int or not 0<=ordinal<len(local):raise ValueError('Ladder candidate input ordinal missing or invalid')
                peak_index=local[ordinal];peak=by_index[peak_index]
                if peak_index in selected:raise ValueError('A ladder peak cannot satisfy two expected labels in one marker')
                label=match.get('label')
                if label!=marker['ladder_labels'][index] or label not in bins:raise ValueError('Matched label differs from expected ladder order')
                if not low<=peak['size_bp']<=high:raise ValueError('Matched source lies outside ladder window')
                if any(f in peak['flags'] for f in ('spike','pullup')):raise ValueError('Matched ladder source has an excluded artifact flag')
                # Coordinates verify integrity after identity resolution; they
                # never select a different peak if the ordinal is inconsistent.
                for key in ('scan','size_bp','height'):
                    if match.get(key)!=peak[key]:raise ValueError('Matched source observations disagree with indexed peak')
                if match.get('center_bp')!=bins[label]['center_bp']:raise ValueError('Matched bin center differs from definition')
                confidence=_number(match.get('confidence'),'ladder initial confidence',0,1e12)
                if confidence!=marker_confidence:raise ValueError('Matched confidence differs from marker matching result')
                if type(match.get('cleared')) is not bool:raise ValueError('Explicit post-match clear-range state required')
                cleared=any(r['min_bp']<=peak['size_bp']<=r['max_bp'] for r in marker['parameters']['clear_ranges'])
                if cleared!=match['cleared']:raise ValueError('Matched clear-range state differs from definition')
                selected[peak_index]=(match,confidence,cleared)
            diagnostics=source.get('diagnostics') or {}
            cutoff=diagnostics.get('cutoff_height');cap=diagnostics.get('top_2n_height')
            reviews=[];calls=[];events=[]
            for peak in linked:
                i=peak['peak_index'];review={**fastcopy(peak),'state':'unmatched_ladder_candidate','reasons':[]}
                artifact=[f for f in ('spike','pullup') if f in peak['flags']]
                if artifact:review.update(state='excluded_artifact',reasons=artifact)
                elif i in selected:
                    match,confidence,cleared=selected[i]
                    if cleared:review.update(state='clear_range',reasons=['clear_range'])
                    else:
                        review['state']='selected'
                        calls.append({**fastcopy(peak),'label':match['label'],'bin_hits':[match['label']],
                            'off_ladder':False,'kind':'ladder_matched','confidence':confidence,
                            'confidence_source':'ladder_ratio_and_unmatched_height_penalty',
                            'expected_bin_center_bp':match['center_bp'],'source_peak_id':peaks[i].get('id'),
                            'review_required':True})
                elif cutoff is not None and peak['height']<cutoff:
                    review.update(state='below_ladder_cutoff',reasons=['below_ladder_cutoff'])
                elif cap is not None and peak['height']<cap:
                    review.update(state='below_top_2n_cutoff',reasons=['below_top_2n_cutoff'])
                else:review['reasons']=['unmatched_ladder_candidate']
                if review['state']!='selected':events.append({'peak_index':i,'reason':review['state']})
                reviews.append(review)
            row.update(status='tentative',window_bp=[low,high],calls=calls,peak_review=reviews,events=events,
                       matching_confidence=source.get('confidence'),cutoff_height=cutoff)
            assigned.update(p['peak_index'] for p in linked)
        except (ValueError,TypeError,OverflowError) as error:
            row['reasons'].append(str(error))
    by_peak={}
    for marker in result['markers']:
        for call in marker['calls']:by_peak.setdefault(call['peak_index'],[]).append((marker,call))
    for index,links in by_peak.items():
        if len(links)>1:
            names=[m['name'] for m,_ in links]
            for marker,call in links:
                if 'cross_marker_overlap' not in call['flags']:call['flags'].append('cross_marker_overlap')
                marker['events'].append({'peak_index':index,'reason':'cross_marker_overlap','markers':names})
    rejected=[m['name'] for m in result['markers'] if m['status']=='rejected']
    if rejected:result['reasons'].append('Rejected ladder observation markers: '+', '.join(rejected))
    if result['markers'] and not rejected:result['status']='tentative'
    elif len(rejected)<len(result['markers']):result['status']='partial'
    result['unassigned_peak_indices']=sorted(set(by_index)-assigned)
    from .ladder_observation_binding import bind
    return bind(peaks,definition,matching,result)
