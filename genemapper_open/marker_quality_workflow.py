"""Explicit, bounded marker-quality annotations on an immutable STR result.

The requested ordinary/AMEL/Y subset is evaluated only with available measured
widths and explicit laboratory settings. It is never substituted for full PQV.
"""
from collections.abc import Mapping
from .spike_adjustment import marker_has_spike
from .fastcopy import fastcopy
import json
from .marker_quality import score_ordinary_marker,score_y_marker,score_negative_marker
from .amel_cross_check import evaluate_amel_cross_check
from .quality_control import quality_category,_number
from .str_workflow import digest


def _object(value,keys,name):
    if not isinstance(value,Mapping) or set(value)!=set(keys):
        raise ValueError(name+': required fields '+', '.join(keys))


def _definition(source):
    try:
        if isinstance(source,(str,bytes)):
            encoded=source.encode('utf-8') if isinstance(source,str) else source
            if len(encoded)>1_048_576:raise ValueError('Marker quality definition exceeds1MiB')
            def pairs(items):
                out={}
                for key,value in items:
                    if key in out:raise ValueError('Duplicate marker quality JSON field: '+key)
                    out[key]=value
                return out
            def invalid(value):raise ValueError('Nonfinite JSON constant: '+value)
            source=json.loads(encoded,object_pairs_hook=pairs,parse_constant=invalid)
        elif len(json.dumps(source,allow_nan=False).encode())>1_048_576:
            raise ValueError('Marker quality definition exceeds1MiB')
    except (TypeError,RecursionError,OverflowError,UnicodeError) as error:
        raise ValueError('Bounded JSON marker quality definition required') from error
    version=source.get('schema_version') if isinstance(source,Mapping) else None
    if type(version) is not int or version not in (1,2):raise ValueError('schema_version must be1 or2')
    keys=('schema_version','sample_sq','gq_thresholds','amel_marker','control_mode','markers')
    _object(source,keys if version==1 else (*keys,'sample_type','amel_cross_check'),'definition')
    modes=('none','positive_attached') if version==1 else ('none','positive_attached','negative_attached')
    if source['control_mode'] not in modes:raise ValueError('Unsupported explicit control_mode')
    if version==2:
        required={'Sample':'none','Positive Control':'positive_attached','Negative Control':'negative_attached'}
        if not isinstance(source['sample_type'],str) or source['sample_type'] not in required:
            raise ValueError('Explicit Sample/Positive Control/Negative Control required')
        if source['control_mode']!=required[source['sample_type']]:raise ValueError('sample_type and control_mode must agree')
        if source['amel_cross_check'] is not None:
            _object(source['amel_cross_check'],('weight',),'amel_cross_check')
            _number(source['amel_cross_check']['weight'],'AMELCC sample weight',0,1)
    amel=source['amel_marker']
    if amel is not None and (not isinstance(amel,str) or not 1<=len(amel)<=128):raise ValueError('Explicit bounded AMEL marker or null required')
    _object(source['gq_thresholds'],('good','bad'),'gq_thresholds')
    quality_category(.5,**source['gq_thresholds'])
    sq=source['sample_sq']
    if not isinstance(sq,Mapping):raise ValueError('Explicit sample_sq source required')
    if sq.get('source') in ('base_quality','post_sizing_quality'):_object(sq,('source',),'sample_sq')
    elif sq.get('source')=='explicit':
        _object(sq,('source','score'),'sample_sq');_number(sq['score'],'sample SQ',0,1)
    else:raise ValueError('sample_sq source must be base_quality, post_sizing_quality or explicit')
    if not isinstance(source['markers'],Mapping) or not 1<=len(source['markers'])<=128:
        raise ValueError('Explicit1..128 marker recipes required')
    for name,recipe in source['markers'].items():
        if not isinstance(name,str) or not 1<=len(name)<=128:raise ValueError('Bounded marker name required')
        negative=version==2 and source['sample_type']=='Negative Control'
        _object(recipe,('kind','spike_metric','spike_weight','confidence_source') if negative else
                ('kind','thresholds','weights','flags','confidence_source'),name)
        if recipe['confidence_source']!='bin_confidence_weight':raise ValueError('Explicit bin_confidence_weight source required')
        if negative:
            if recipe['kind'] not in ('autosomal','amel','y'):raise ValueError('Explicit autosomal/amel/y kind required')
            score_negative_marker([],zero_peak_value=1,spike_metric=recipe['spike_metric'],
                                  spike_weight=recipe['spike_weight'],sample_sq=1)
            continue
        arguments=([],recipe['thresholds'],recipe['weights'],recipe['flags'])
        if recipe['kind']=='y':score_y_marker(*arguments,sample_sq=1,amel_calls=None,control_mismatch=False)
        else:score_ordinary_marker(*arguments,sample_sq=1,marker_kind=recipe['kind'],control_mismatch=False)
    return fastcopy(dict(source))


def evaluate_marker_quality(report,definition):
    definition=_definition(definition)
    version=definition['schema_version']
    negative=version==2 and definition['sample_type']=='Negative Control'
    if not isinstance(report,Mapping) or not isinstance(report.get('allele_calling'),Mapping):
        raise ValueError('Completed STR report required')
    markers=report['allele_calling'].get('markers')
    calls,peaks=report.get('calls'),report.get('peaks')
    if not isinstance(markers,list) or len(markers)>128 or not isinstance(calls,list) or len(calls)>4096 or not isinstance(peaks,list) or len(peaks)>4096:
        raise ValueError('Bounded STR marker/call/peak observations required')
    by_peak={peak['id']:peak for peak in peaks}
    if len(by_peak)!=len(peaks):raise ValueError('Unique source peak IDs required')
    by_marker={marker['name']:marker for marker in markers}
    if len(by_marker)!=len(markers):raise ValueError('Unique marker names required')
    sq=None;global_reason=None
    if definition['sample_sq']['source']=='explicit':sq=float(definition['sample_sq']['score'])
    else:
        sizing=report.get('sizing',{})
        key='post_quality' if definition['sample_sq']['source']=='post_sizing_quality' else 'base_quality'
        base=sizing.get(key) or {}
        if sizing.get('status')=='accepted' and base.get('status')=='evaluated':
            sq=_number(base.get('score'),'base sizing quality',0,1)
        else:global_reason='accepted_post_sizing_quality_missing' if key=='post_quality' else 'accepted_base_quality_missing'
    controls=(report.get('quality') or {}).get('controls')
    if controls and controls.get('sample_type')=='Negative Control' and not negative:
        global_reason='negative_control_gq_requires_schema2' if version==2 else 'negative_control_gq_not_implemented'
    if definition['control_mode'] in ('positive_attached','negative_attached'):
        expected_type='Negative Control' if negative else 'Positive Control'
        prefix='negative' if negative else 'positive'
        if not controls or controls.get('sample_type')!=expected_type:global_reason=prefix+'_control_observations_missing'
        elif controls.get('input_observations_sha256')!=digest(report['allele_calling']):global_reason=prefix+'_control_observations_stale'
    control_rows={row['marker']:row for row in controls['markers']} if controls else {}
    amel_calls=None;amel_reason=None
    if definition['amel_marker'] is not None and not negative:
        amel=by_marker.get(definition['amel_marker'])
        if amel is None or amel.get('status')!='tentative':amel_reason='explicit_amel_marker_unavailable'
        else:amel_calls=[call['label'] for call in amel['calls']]
    result={'schema_version':1,'definition_schema_version':version,'status':'unavailable','definition_sha256':digest(definition),
        'control_mode':definition['control_mode'],'sample_sq_source':definition['sample_sq']['source'],
        'sample_sq':sq,'markers':[],'sample_category':None,'all_markers_evaluated':False,
        'complete_for_requested_subset':False,'external_validation':False,'original_runtime_validated':False,
        'full_original_pqv':False,'not_applied':['AMELCC plugin','ladder GQ','pullup processor',
            'overrides and stale original project states','original floating-point rounding parity']}
    if not negative:result['not_applied'].append('negative-control GQ')
    for marker in markers:
        name=marker['name'];recipe=definition['markers'].get(name)
        row={'marker':name,'status':'unavailable','reason':None,'score':None,'category':None,'call_scores':[]}
        reason=global_reason
        if marker.get('status')!='tentative':reason='marker_analysis_rejected'
        elif recipe is None:reason='explicit_marker_quality_recipe_missing'
        elif recipe['kind']=='y' and amel_reason and not negative:reason=amel_reason
        selected=[call for call in calls if call.get('marker')==name]
        observations=[]
        if reason is None:
            expected=[(call['peak_index'],call['label'],call['size_bp']) for call in marker['calls']]
            actual=[(call['peak_index'],call['label'],call['size_bp']) for call in selected]
            if actual!=expected:raise ValueError(name+': inconsistent root and marker calls')
            for call in selected:
                if negative:
                    observations.append(_number(call['confidence_weight'],'initial confidence',0,1));continue
                peak=by_peak.get(call['source_peak_id'])
                morphology=(peak or {}).get('morphology_bp',{})
                if morphology.get('status')!='evaluated' or type((peak or {}).get('offscaled')) is not bool:
                    reason='calibrated_morphology_or_offscale_state_missing';break
                observations.append({'label':call['label'],'height':call['height'],
                    'fwhm_bp':morphology['fwhm_bp'],'offscale':peak['offscaled'],'confidence':call['confidence_weight']})
        control=control_rows.get(name) if definition['control_mode']!='none' else None
        if reason is None and definition['control_mode']!='none' and control is None:
            reason='marker_control_observation_missing'
        if reason is None and negative and (not control.get('comparison_performed') or 'zero_peak_value' not in control):
            reason='negative_zero_peak_observation_missing'
        if reason is not None:
            row['reason']=reason;result['markers'].append(row);continue
        effective_flags=fastcopy(recipe.get('flags',{}))
        observed_flags={'binning':any(call.get('off_ladder') is True for call in selected),
                        'overlap':any('cross_marker_overlap' in call.get('flags',[]) for call in selected),
                        'spike':marker_has_spike(marker)}
        if negative:
            scored=score_negative_marker(observations,zero_peak_value=control['zero_peak_value'],
                spike_metric=1 if observed_flags['spike'] else recipe['spike_metric'],spike_weight=recipe['spike_weight'],sample_sq=sq)
        else:
            for key,value in observed_flags.items():effective_flags[key]=effective_flags[key] or value
            common={'sample_sq':sq,'control_mismatch':bool(control and control['cc']==1)}
            arguments=(observations,recipe['thresholds'],recipe['weights'],effective_flags)
            scored=(score_y_marker(*arguments,amel_calls=amel_calls,**common) if recipe['kind']=='y'
                    else score_ordinary_marker(*arguments,marker_kind=recipe['kind'],**common))
        category=quality_category(scored['gq'],**definition['gq_thresholds'])
        row.update(status='evaluated_subset',score=scored,category=category,
            flag_sources={'explicit':recipe.get('flags'),'observed_call_violations':observed_flags,'effective':effective_flags},
            control_comparison_performed=control['comparison_performed'] if control else None,
            call_scores=[{'call_id':call['id'],'before_sample_sq':before,'after_sample_sq':after}
                for call,before,after in zip(selected,scored['call_confidence_before_sq'],scored['call_confidence_after_sq'])])
        result['markers'].append(row)
    plugin_complete=True
    plugin=definition.get('amel_cross_check')
    if plugin is not None:
        if any(m['name'] not in definition['markers'] for m in markers):
            cross={'status':'unavailable','reason':'explicit_panel_marker_roles_missing','updates':[]}
        else:
            cross=evaluate_amel_cross_check([{'name':marker['name'],
                'is_y_marker':definition['markers'][marker['name']]['kind']=='y',
                'calls':[{'label':c['label'],'off_ladder':c['off_ladder']} for c in marker['calls']],
                'gq':row['score']['gq'] if row['score'] else None}
                for marker,row in zip(markers,result['markers'])],sample_type=definition['sample_type'],weight=plugin['weight'])
        result['amel_cross_check']=cross
        plugin_complete=cross['status']!='unavailable'
        if plugin_complete:result['not_applied'].remove('AMELCC plugin')
        updates={u['marker']:u for u in cross['updates']}
        for row in result['markers']:
            if row['marker'] not in updates:continue
            update=updates[row['marker']];score=row['score']
            score['gq_before_amel_cc']=score['gq'];score['gq']=update['gq_after']
            score['scope']='sample_marker_with_amel_cross_check_without_overrides_or_pullup_processor'
            score['metrics']['amel_cross_check']=update['metric']
            score['events'].append({'criterion':'amel_cross_check','metric':update['metric'],
                'gq_before':update['gq_before'],'gq_after':update['gq_after'],
                'call_confidences_recomputed':False})
            row['category']=quality_category(score['gq'],**definition['gq_thresholds'])
    evaluated=[row for row in result['markers'] if row['status']=='evaluated_subset']
    result['all_markers_evaluated']=bool(markers) and len(evaluated)==len(markers)
    result['complete_for_requested_subset']=result['all_markers_evaluated'] and plugin_complete and all(
        row.get('control_comparison_performed') is not False for row in evaluated)
    if evaluated:
        result['status']='evaluated_subset' if result['all_markers_evaluated'] and plugin_complete else 'partial'
        result['sample_category']=max(row['category']['code'] for row in evaluated)
    dependency={key:report.get(key) for key in ('calls','peaks','allele_calling','sizing')}
    if definition['control_mode']!='none':dependency['controls']=controls
    result['input_observations_sha256']=digest(dependency)
    return result


def attach_marker_quality(report,definition):
    result=fastcopy(report)
    quality=evaluate_marker_quality(report,definition)
    result.setdefault('quality',{})['marker_gq']=quality
    result.setdefault('settings',{})['marker_quality_definition_sha256']=quality['definition_sha256']
    return result
