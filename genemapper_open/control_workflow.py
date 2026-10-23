"""Explicit control checks on fresh independent STR workflow observations.

This is not a full original GQ/PQV processor or an authenticated control
definition store. Missing expected profiles remain visible as skipped checks.
"""
from collections.abc import Mapping
from copy import deepcopy
import json
from .quality_control import (positive_control,negative_control,zero_peak_metric,
                              composite_control_category)
from .str_workflow import digest


def evaluate_controls(report,definition):
    if isinstance(definition,(str,bytes)):
        if len(definition)>1_048_576:raise ValueError('Control definition exceeds1MiB')
        def pairs(items):
            result={}
            for key,value in items:
                if key in result:raise ValueError('Duplicate control definition field')
                result[key]=value
            return result
        definition=json.loads(definition,object_pairs_hook=pairs)
    if not isinstance(definition,Mapping) or set(definition)!= {'schema_version','sample_type','markers'}:
        raise ValueError('Explicit schema_version/sample_type/markers control definition required')
    if type(definition['schema_version']) is not int or definition['schema_version']!=1:
        raise ValueError('Control definition schema_version must be1')
    kind=definition['sample_type']
    if kind not in ('Positive Control','Negative Control','Sample'):
        raise ValueError('Choose exact Positive Control, Negative Control or Sample')
    definitions=definition['markers']
    if not isinstance(definitions,Mapping) or len(definitions)>128:
        raise ValueError('At most128 explicit marker definitions required')
    if not isinstance(report,Mapping) or not isinstance(report.get('allele_calling'),Mapping):
        raise ValueError('A completed STR workflow report is required')
    markers=report['allele_calling'].get('markers')
    if not isinstance(markers,list) or len(markers)>128:raise ValueError('Bounded marker list required')
    outputs=[]
    for marker in markers:
        name=marker['name'];settings=definitions.get(name)
        output={'marker':name,'cc':-2,'gq_forced_zero':False,'comparison_performed':False,'reason':None}
        if marker.get('status')!='tentative':
            output['reason']='marker_analysis_rejected'
        elif kind=='Sample':
            output['reason']='not_a_control'
        elif settings is None:
            output['reason']='explicit_marker_control_definition_missing'
        else:
            if not isinstance(settings,Mapping) or set(settings)!= {'expected_alleles','quality_sensor','is_y_marker'}:
                raise ValueError(name+': expected_alleles/quality_sensor/is_y_marker required')
            if type(settings['quality_sensor']) is not bool or type(settings['is_y_marker']) is not bool:
                raise ValueError(name+': explicit sensor/Y booleans required')
            called=[call['label'] for call in marker['calls']]
            if kind=='Positive Control':
                output.update(positive_control(called,settings['expected_alleles'],is_y_marker=settings['is_y_marker']))
            else:
                reviews=marker.get('peak_review')
                if not isinstance(reviews,list):raise ValueError(name+': pre-cutoff peak review required')
                candidates=sum(not any(flag in peak.get('flags',[]) for flag in ('spike','pullup')) for peak in reviews)
                zero=zero_peak_metric(candidates,called,quality_sensor=settings['quality_sensor'],expected=settings['expected_alleles'])
                output.update(negative_control(zero['value']),zero_peak=zero,comparison_performed=True,reason='negative_zero_peak_rule')
        outputs.append(output)
    composite=composite_control_category([output['cc'] for output in outputs])
    return {'schema_version':1,'sample_type':kind,'definition_sha256':digest(dict(definition)),
        'input_observations_sha256':digest(report['allele_calling']), 'markers':outputs,'composite_control':composite,
        'all_control_comparisons_performed':bool(outputs) and all(row['comparison_performed'] for row in outputs),
        'external_validation':False,'original_runtime_validated':False,
        'not_applied':['original full GQ weights','spike penalty','custom profile selection/UI validation',
                       'stale results or overrides','original case-sensitive sample type entry routes']}


def attach_control_result(report,definition):
    """Return a new immutable-machine candidate, preserving preliminary status."""
    result=deepcopy(report)
    quality=evaluate_controls(report,definition)
    prior_controls=(result.get('quality') or {}).get('controls')
    result.setdefault('quality',{})['controls']=quality
    marker_quality=result['quality'].get('marker_gq')
    route_changed=(prior_controls or {}).get('sample_type')!=quality['sample_type'] and (
        quality['sample_type']=='Negative Control' or (prior_controls or {}).get('sample_type')=='Negative Control')
    if marker_quality and prior_controls!=quality and (
            marker_quality.get('control_mode') in ('positive_attached','negative_attached') or route_changed):
        marker_quality.update(status='stale',stale_reason='control_observations_changed',
            sample_category=None,complete_for_requested_subset=False,all_markers_evaluated=False)
    result['settings']=deepcopy(result.get('settings',{}))
    result['settings']['control_definition_sha256']=quality['definition_sha256']
    return result
