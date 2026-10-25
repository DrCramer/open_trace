"""Explicit fresh single-ladder QC and whole-ladder offset admission.

The observed ladder path consumes matched calls and all linked window peaks.
Legacy kit_sample_parameters is a separately declared comparison policy. The
independent matching optimizer and remaining AL/PQV limitations stay explicit.
"""
from collections.abc import Mapping
from .fastcopy import fastcopy
import json
import math
from .allele_calling import call_sample
from .kit_definition import load_kit_definition
from .ladder_quality import _object,validate_ladder_recipe,score_ladder_marker
from .quality_control import _number,quality_category
from .str_workflow import digest
from .spike_adjustment import marker_has_spike


def load_ladder_quality_definition(source):
    try:
        if isinstance(source,(str,bytes)):
            encoded=source.encode('utf-8') if isinstance(source,str) else source
            if len(encoded)>1_048_576:raise ValueError('Ladder quality definition exceeds1MiB')
            def pairs(items):
                result={}
                for key,value in items:
                    if key in result:raise ValueError('Duplicate ladder quality JSON field: '+key)
                    result[key]=value
                return result
            def invalid(value):raise ValueError('Nonfinite JSON constant: '+value)
            source=json.loads(encoded,object_pairs_hook=pairs,parse_constant=invalid)
        elif len(json.dumps(source,allow_nan=False).encode())>1_048_576:
            raise ValueError('Ladder quality definition exceeds1MiB')
    except (TypeError,RecursionError,OverflowError,UnicodeError) as error:
        raise ValueError('Bounded ladder quality JSON required') from error
    _object(source,('schema_version','calling_parameters_source','ladder_sq','sq_thresholds','gq_thresholds','markers'),'ladder quality definition')
    if type(source['schema_version']) is not int or source['schema_version']!=1:raise ValueError('Ladder quality schema_version must be1')
    if source['calling_parameters_source'] not in ('kit_sample_parameters','ladder_matching_calls'):
        raise ValueError('Explicit ladder_matching_calls or legacy kit_sample_parameters policy required')
    for key in ('sq_thresholds','gq_thresholds'):
        _object(source[key],('good','bad'),key);quality_category(.5,**source[key])
    sq=source['ladder_sq']
    if not isinstance(sq,Mapping):raise ValueError('Explicit ladder_sq source required')
    if sq.get('source') in ('base_quality','post_sizing_quality'):_object(sq,('source',),'ladder_sq')
    elif sq.get('source')=='explicit':
        _object(sq,('source','score'),'ladder_sq');_number(sq['score'],'explicit ladder SQ',0,1)
    else:raise ValueError('ladder_sq source must be base_quality, post_sizing_quality or explicit')
    if not isinstance(source['markers'],Mapping) or not 1<=len(source['markers'])<=128:
        raise ValueError('Explicit1..128 marker recipes required')
    for name,recipe in source['markers'].items():
        if not isinstance(name,str) or not 1<=len(name)<=128:raise ValueError('Bounded marker name required')
        validate_ladder_recipe(recipe)
    return fastcopy(dict(source))


def gate_ladder_offsets(peaks,kit,matching,definition,*,sizing,missing_offset_policy='reject',call_observations=None):
    """Return (effective offsets, quality). Any missing/failed QC withholds all.

    Matching remains visible in matched/proposed rows. Successful matching alone
    cannot admit offsets when an explicit quality definition has been requested.
    """
    definition=load_ladder_quality_definition(definition)
    kit=load_kit_definition(kit)
    known={m['name'] for m in kit['markers']}
    if set(definition['markers'])-known:raise ValueError('Ladder quality recipe references unknown kit marker')
    policy=definition['calling_parameters_source']
    if call_observations is None:
        if policy=='ladder_matching_calls':
            from .ladder_calling import ladder_call_observations
            observations=ladder_call_observations(peaks,kit,matching)
        else:
            from .ladder_observation_binding import bind
            observations=bind(peaks,kit,matching,
                call_sample(peaks,kit,matching,sizing_accepted=True,missing_offset_policy=missing_offset_policy))
    else:
        if not isinstance(call_observations,Mapping):raise ValueError('Ladder call observations must be an object')
        observations=fastcopy(call_observations)
    expected_method='ladder_matching_calls' if policy=='ladder_matching_calls' else 'explicit_kit_ordered_sample_filters'
    if observations.get('diagnostics',{}).get('method')!=expected_method:
        raise ValueError('Ladder call observation source differs from requested policy')
    from .ladder_observation_binding import validate
    observations=validate(peaks,kit,matching,observations)
    by_marker={r['name']:r for r in observations['markers']}
    sq=None;global_reason=None
    if definition['ladder_sq']['source']=='explicit':sq=float(definition['ladder_sq']['score'])
    else:
        key='post_quality' if definition['ladder_sq']['source']=='post_sizing_quality' else 'base_quality'
        value=sizing.get(key) or {} if isinstance(sizing,Mapping) else {}
        if isinstance(sizing,Mapping) and sizing.get('status')=='accepted' and value.get('status')=='evaluated':
            sq=_number(value.get('score'),'ladder sizing quality',0,1)
        else:global_reason='accepted_ladder_post_sizing_quality_missing' if key=='post_quality' else 'accepted_ladder_base_quality_missing'
    result={'schema_version':1,'status':'unavailable','definition_sha256':digest(definition),
        'scope':'fresh_single_ladder_gq_and_sq_subset_with_explicit_call_observation_policy',
        'calling_parameters_source':definition['calling_parameters_source'],'ladder_sq_source':definition['ladder_sq']['source'],
        'ladder_sq':sq,'sq_category':quality_category(sq,**definition['sq_thresholds']) if sq is not None else None,
        'markers':[],'all_markers_evaluated':False,'gq_category':None,'al_subset_code':None,
        'gate_passed':False,'full_original_al':False,'external_validation':False,'original_runtime_validated':False,
        'not_applied':['automatic spike/pullup detectors','original full SQ/PQV composition',
            'original ladder call selection equivalence','overrides, null-peak states and original rounding']}
    for marker in kit['markers']:
        name=marker['name'];recipe=definition['markers'].get(name);observed=by_marker.get(name)
        row={'marker':name,'status':'unavailable','reason':None,'score':None,'category':None,'calls':[]}
        reason=global_reason
        if (matching.get('markers',{}).get(name) or {}).get('status')!='accepted':reason='ladder_matching_not_accepted'
        elif observed is None or observed['status']!='tentative':reason='ladder_call_observations_unavailable'
        elif recipe is None:reason='explicit_ladder_quality_recipe_missing'
        effective=fastcopy(recipe)
        if reason is None:
            reviews=observed['peak_review']
            indices=[p['peak_index'] for p in reviews]
            if any(type(peaks[i].get('offscaled')) is not bool for i in indices):reason='ladder_offscale_state_missing'
            else:
                seen_spike=marker_has_spike(observed)
                seen_offscale=any(peaks[i]['offscaled'] for i in indices) or any('offscale' in p.get('flags',[]) for p in reviews)
                if seen_spike:effective['spike_metric']=1
                if seen_offscale:effective['offscale_metric']=1
                row['observed_flags']={'spike':seen_spike,'offscale':seen_offscale}
        if reason is not None:
            row['reason']=reason;result['markers'].append(row);continue
        bins={b['label']:b for b in marker['bins']}
        for call in observed['calls']:
            label=call['label'];bin_=bins.get(label) if not call['off_ladder'] else None
            row['calls'].append({'name':label,'size_bp':call['size_bp'],'peak_bp':call['size_bp'],
                'height':call['height'],'off_ladder':call['off_ladder'],
                'bin':{'name':label,'center_bp':bin_['center_bp']} if bin_ is not None else None})
        score=score_ladder_marker(row['calls'],marker['ladder_labels'],effective)
        row.update(status='evaluated_subset',score=score,category=quality_category(score['gq'],**definition['gq_thresholds']))
        source_key='confidence' if policy=='ladder_matching_calls' else 'confidence_weight'
        initial=[_number(c.get(source_key),'ladder initial confidence',0,1e12) for c in observed['calls']]
        mean=math.fsum(initial)/len(initial) if initial else None
        row['initial_confidence_mean']=mean
        row['confidence_source']='ladder_ratio_and_unmatched_height_penalty' if policy=='ladder_matching_calls' else 'legacy_bin_confidence_weight'
        row['call_scores']=[{'peak_index':c['peak_index'],'label':c['label'],'initial_confidence':value,
            'after_marker_gq':mean*score['gq'],'after_sample_sq':mean*score['gq']*sq}
            for c,value in zip(observed['calls'],initial)]
        result['markers'].append(row)
    evaluated=[row for row in result['markers'] if row['status']=='evaluated_subset']
    result['all_markers_evaluated']=bool(result['markers']) and len(evaluated)==len(result['markers'])
    if evaluated:result['status']='evaluated_subset' if result['all_markers_evaluated'] else 'partial'
    if result['all_markers_evaluated']:
        result['gq_category']=max(row['category']['code'] for row in evaluated)
        # Original fresh AL composition: default2, translated SQ only if all GQ pass.
        result['al_subset_code']=result['sq_category']['code'] if result['gq_category']==0 else 2
        result['gate_passed']=result['al_subset_code']==0
    result['input_observations_sha256']=digest({'peaks':peaks,'kit':kit,'matching':matching,
                                               'sizing':sizing,'calling':observations})
    if 'spike_adjustments' in observations:
        result['spike_adjustments']=fastcopy(observations['spike_adjustments'])
        result['not_applied'][0]='automatic pullup detector'
    effective_offsets=fastcopy(matching)
    effective_offsets['quality_gate']={key:result[key] for key in ('status','definition_sha256','al_subset_code','gate_passed','full_original_al')}
    for record in effective_offsets.get('markers',{}).values():
        record['matching_status_before_quality']=record['status']
        if not result['gate_passed']:
            record.update(status='rejected',bin_offsets={},min_offset_bp=None,max_offset_bp=None)
            record.setdefault('reasons',[]).append('Requested whole-ladder quality subset did not admit offsets')
    if not result['gate_passed']:
        effective_offsets['status']='rejected'
        effective_offsets.setdefault('reasons',[]).append('Requested whole-ladder quality subset did not admit offsets')
    return effective_offsets,result
