"""Explicit pairwise pull-up reconstruction; original route is unconfirmed.

The retained private ar body supplies the pair/list rules. Sorting, cross-dye
routes and processing phase are our explicitly requested orchestration policy.
No implicit all-vs-all detector, laboratory defaults or original-runtime claim.
"""
from collections.abc import Mapping
from copy import deepcopy
import json
import math
import re
from .quality_control import _number,_bool


def load_pullup_definition(source):
    try:
        if isinstance(source,(str,bytes)):
            encoded=source.encode('utf-8') if isinstance(source,str) else source
            if len(encoded)>16384:raise ValueError('Pull-up definition exceeds16KiB')
            def pairs(items):
                result={}
                for key,value in items:
                    if key in result:raise ValueError('Duplicate pull-up field: '+key)
                    result[key]=value
                return result
            def invalid(value):raise ValueError('Nonfinite pull-up JSON constant: '+value)
            source=json.loads(encoded,object_pairs_hook=pairs,parse_constant=invalid)
        elif len(json.dumps(source,allow_nan=False).encode())>16384:
            raise ValueError('Pull-up definition exceeds16KiB')
    except (TypeError,RecursionError,OverflowError,UnicodeError) as error:
        raise ValueError('Bounded pull-up definition JSON required') from error
    if not isinstance(source,Mapping) or set(source)!={'schema_version','detector','profile_threshold'}:
        raise ValueError('Explicit pull-up detector/profile fields required')
    if type(source['schema_version']) is not int or source['schema_version']!=1:
        raise ValueError('Pull-up schema_version must be1')
    _number(source['profile_threshold'],'profile_threshold',0,1)
    detector=source['detector']
    if detector is not None:
        fields={'algorithm','calling_policy','phase','reference_order','max_scan_distance',
                'height_ratio_threshold','channel_pairs'}
        if not isinstance(detector,Mapping) or set(detector)!=fields:raise ValueError('All explicit detector fields required')
        expected={'algorithm':'static_ar_pairwise_body','calling_policy':'explicit_ordered_channel_pairs',
            'phase':'after_spike_restoration_before_calling','reference_order':'stable_scan_midpoint_binary_search'}
        if any(detector[key]!=value for key,value in expected.items()):raise ValueError('Unsupported explicit pull-up policy')
        if type(detector['max_scan_distance']) is not int or not 0<=detector['max_scan_distance']<=1_000_000:
            raise ValueError('max_scan_distance must be an integer in[0,1000000]')
        _number(detector['height_ratio_threshold'],'height_ratio_threshold',0,1)
        routes=detector['channel_pairs']
        if not isinstance(routes,list) or not 1<=len(routes)<=32:raise ValueError('Explicit1..32 ordered channel pairs required')
        for route in routes:
            if not isinstance(route,Mapping) or set(route)!={'candidate_channel','reference_channel'}:
                raise ValueError('Candidate/reference channel pair required')
            if any(not isinstance(tag,str) or not re.fullmatch(r'DATA[0-9]{1,6}',tag) for tag in route.values()):
                raise ValueError('Explicit DATA channel tags required')
            if route['candidate_channel']==route['reference_channel']:raise ValueError('Cross-channel route requires different dyes')
    return deepcopy(dict(source))


def _flag(peak,name):
    value=peak.get(name,False);_bool(value,name)
    flags=peak.get('flags',[])
    if isinstance(flags,Mapping):
        if any(not isinstance(key,str) or type(v) is not bool for key,v in flags.items()):raise ValueError('Boolean flag map required')
        return value or flags.get(name,False)
    if not isinstance(flags,(list,tuple)) or any(not isinstance(v,str) for v in flags):raise ValueError('String flag list required')
    return value or name in flags


def _state(peak):
    label=peak.get('artifact_label')
    if label is not None and (not isinstance(label,str) or len(label)>128):raise ValueError('Bounded artifact label required')
    return {'pullup':_flag(peak,'pullup'),
        'ratio':_number(peak.get('pullup_ratio',-1),'stored pull-up ratio',-1e12,1e12),
        'ratio_threshold':_number(peak.get('pullup_ratio_threshold',-1),'stored pull-up threshold',-1,1e12),
        'artifact_label':label,'peak_called_state':None,'originally_artifact':None}


def _full_set(state,value,ratio=None,threshold=None):
    """Modeled full setter effects, separate from a flag-only edit."""
    state['pullup']=value;state['peak_called_state']=1;state['originally_artifact']=True
    if not value:state['artifact_label']=''
    elif state['artifact_label'] is None or all(ord(c)<=32 for c in state['artifact_label']):state['artifact_label']='SPU'
    if ratio is not None:state.update(ratio=ratio,ratio_threshold=threshold)


def _binary_search(references,scan):
    low,high=0,len(references)-1
    while low<=high:
        middle=(low+high)//2
        value=references[middle]['scan']
        if value<scan:low=middle+1
        elif value>scan:high=middle-1
        else:return middle,True
    return low,False


def _attempt(candidate,reference,state,settings):
    before=deepcopy(state);_full_set(state,False)
    height=reference['height']
    # The original divides before its positive-reference guard. A zero
    # reference produces an unused IEEE nonfinite ratio; finite JSON uses null.
    ratio=candidate['height']/height if height!=0 else None
    finite=ratio is not None and math.isfinite(ratio)
    if height!=0 and not finite:raise ValueError('Nonfinite pull-up division is outside the finite adapter')
    distance=abs(reference['scan']-candidate['scan'])
    matched=height>0 and distance<=settings['max_scan_distance'] and finite and ratio<settings['height_ratio_threshold']
    if matched:_full_set(state,True,ratio,settings['height_ratio_threshold'])
    reason=('reference_height_not_positive' if height<=0 else 'distance_above_threshold'
        if distance>settings['max_scan_distance'] else 'ratio_not_below_threshold' if not matched else 'matched')
    return {'reference_id':reference['_pullup_id'],'distance_scan':distance,
        'attempt_ratio':ratio if finite else None,'ratio_available':finite,
        'matched':matched,'reason':reason,'before':before,'after':deepcopy(state)}


def apply_pullup_detection(channels,definition,*,selected_group,reference_channel):
    """Copy selected peak lists only; leave waveform arrays and reference read-only.

    Candidate channels and reference channels must be explicitly selected in
    the same DATA group. Reference flags are deliberately not filtered. The
    caller must start from fresh analysis, not apply the detector twice.
    """
    definition=load_pullup_definition(definition)
    if selected_group not in ('raw','analyzed'):raise ValueError('Selected raw/analyzed group required')
    if not isinstance(reference_channel,str):raise ValueError('Explicit size-standard channel required')
    if not isinstance(channels,list) or len(channels)>128:raise ValueError('Bounded channel list required')
    settings=definition['detector']
    if settings is None:
        return {'peaks_by_channel':{},'summary':{'status':'not_requested','scope':'existing_flags_only',
            'original_automatic_route_confirmed':False}}
    chosen={};all_tags=set();total=0
    for channel in channels:
        if not isinstance(channel,Mapping) or not isinstance(channel.get('tag'),str):raise ValueError('Channel tag required')
        tag=channel['tag']
        if tag in all_tags:raise ValueError('Duplicate DATA channel')
        all_tags.add(tag)
        if channel.get('group')!=selected_group:continue
        rows=channel.get('peaks')
        if not isinstance(rows,list) or len(rows)>4096:raise ValueError('At most4096 measured peaks per channel')
        total+=len(rows)
        if total>32768:raise ValueError('At most32768 selected-group peaks required')
        prepared=[]
        for index,peak in enumerate(rows):
            if not isinstance(peak,Mapping):raise ValueError('Measured peak mapping required')
            if 'pullup_classification' in peak:raise ValueError('Fresh FSA analysis required for pull-up detection')
            _number(peak.get('scan'),'scan',0,1e8);_number(peak.get('height'),'height',-1e12,1e12)
            _bool(peak.get('offscaled',False),'offscaled');_flag(peak,'spike');_state(peak)
            prepared.append({**deepcopy(peak),'_pullup_id':f'{tag}:{index}'})
        chosen[tag]=prepared
    for route in settings['channel_pairs']:
        a,b=route['candidate_channel'],route['reference_channel']
        if a==reference_channel:raise ValueError('Size-standard channel cannot be a pull-up candidate')
        if a not in chosen or b not in chosen:raise ValueError('Pull-up routes must use channels in the selected DATA group')
    states={p['_pullup_id']:_state(p) for peaks in chosen.values() for p in peaks}
    histories={key:{'status':'not_targeted','initial_state':deepcopy(state),'events':[],
        'skips':[],'matched_reference_id':None,'original_automatic_route_confirmed':False}
        for key,state in states.items()}
    passes=[];attempt_count=0
    for order,route in enumerate(settings['channel_pairs']):
        candidates=chosen[route['candidate_channel']]
        references=sorted(chosen[route['reference_channel']],key=lambda p:p['scan'])
        attempted=skipped=matched=0
        for candidate in candidates:
            identifier=candidate['_pullup_id'];state=states[identifier];history=histories[identifier]
            history['status']='evaluated_explicit_passes'
            if not references:
                history['skips'].append({'pass_index':order,'reason':'reference_list_empty'});continue
            reason='candidate_offscale' if candidate.get('offscaled',False) else 'candidate_already_pullup' if state['pullup'] else 'candidate_current_spike' if _flag(candidate,'spike') else None
            if reason:
                skipped+=1;history['skips'].append({'pass_index':order,'reason':reason});continue
            position,exact=_binary_search(references,candidate['scan'])
            indices=([position] if exact else [0] if position==0 else [len(references)-1]
                if position==len(references) else [position,position-1])
            for index in indices:
                event=_attempt(candidate,references[index],state,settings)
                event.update(pass_index=order,binary_search_position=position,exact_scan_match=exact,
                    reference_sorted_index=index)
                history['events'].append(event);attempted+=1;attempt_count+=1
                if attempt_count>1_000_000:raise ValueError('Pull-up comparison work budget exceeded')
                if state['pullup']:
                    history['matched_reference_id']=references[index]['_pullup_id'];matched+=1;break
        passes.append({'pass_index':order,**route,'candidate_count':len(candidates),'reference_count':len(references),
            'attempted_pairs':attempted,'skipped_candidates':skipped,'new_pullups':matched,
            'duplicate_reference_scans':len(references)-len({p['scan'] for p in references})})
    output={}
    # The size-standard list was never a candidate; return no replacement for
    # it so even incidental metadata cannot alter reference sizing observations.
    for tag,peaks in chosen.items():
        if tag==reference_channel:continue
        output[tag]=[]
        for peak in peaks:
            identifier=peak.pop('_pullup_id');state=states[identifier];history=histories[identifier]
            history.update(state=deepcopy(state),scope='explicit_cross_channel_body_reconstruction',
                ratio_provenance='measured_pair' if history['matched_reference_id'] is not None else 'supplied_or_fresh_state')
            peak.update(pullup=state['pullup'],pullup_classification=history)
            output[tag].append(peak)
    return {'peaks_by_channel':output,'summary':{'status':'evaluated_explicit_passes',
        'scope':'experimental_static_body_with_own_ordered_routes','original_automatic_route_confirmed':False,
        'phase':settings['phase'],'attempted_pairs':attempt_count,'passes':passes,
        'current_pullups':sum(state['pullup'] for key,state in states.items() if not key.startswith(reference_channel+':'))}}
