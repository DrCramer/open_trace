"""Persistent run revisions over compact sized observations.

This independent policy calculates new immutable results for every registered
sample. Prior machine results and manual reviews stay attached to their prior
analysis. Original dependency invalidation informs this design; the original
GUI/state scheduler is not reproduced or invoked.
"""
from collections.abc import Mapping
from copy import deepcopy
import json
from uuid import uuid4
from .kit_definition import load_kit_definition
from .ladder_quality_workflow import load_ladder_quality_definition
from .project_store import _canonical,_digest,_text
from .str_workflow import compact_analysis
from .run_workflow import run_str_batch_workflow,_ladder_entries
from .quality_control import _labels

MAX_RUN_INPUT_BYTES=16*1024*1024


def prepare_run_inputs(source):
    if isinstance(source,(str,bytes)):
        encoded=source.encode('utf-8') if isinstance(source,str) else source
        if len(encoded)>MAX_RUN_INPUT_BYTES:raise ValueError('Run inputs exceed16MiB')
        def pairs(items):
            result={}
            for key,value in items:
                if key in result:raise ValueError('Duplicate run input field')
                result[key]=value
            return result
        source=json.loads(encoded,object_pairs_hook=pairs)
    fields={'schema_version','kit','ladder_quality_definition','missing_offset_policy','ladders','samples'}
    if not isinstance(source,Mapping) or set(source)!=fields or type(source['schema_version']) is not int or source['schema_version']!=1:
        raise ValueError('Explicit version1 run inputs with all fields required')
    if source['missing_offset_policy'] not in ('reject','forward_fill'):raise ValueError('Invalid missing offset policy')
    ladders=source['ladders'];samples=source['samples']
    if not isinstance(ladders,list) or len(ladders)>128:raise ValueError('Explicit0..128 ladders required')
    if ladders:_ladder_entries(ladders,'analysis')
    if not isinstance(samples,list) or len(samples)>128:raise ValueError('Explicit0..128 samples required')
    normalized={'schema_version':1,'kit':load_kit_definition(source['kit']),
        'ladder_quality_definition':load_ladder_quality_definition(source['ladder_quality_definition']),
        'missing_offset_policy':source['missing_offset_policy'],'ladders':[],'samples':[]}
    names=set()
    for entry in ladders:
        folded=entry['ladder_id'].lower()
        if folded in names:raise ValueError('Persistent run ladder names must be unique ignoring ASCII case')
        names.add(folded)
        if not isinstance(entry['analysis'],Mapping):raise ValueError('Sized ladder report required')
        normalized['ladders'].append({'ladder_id':entry['ladder_id'],'analysis':compact_analysis(entry['analysis'])})
    keys=set()
    for entry in samples:
        allowed={'sample_key','analysis','marker_quality_definition','control_definition'}
        if not isinstance(entry,Mapping) or not {'sample_key','analysis'}<=set(entry) or set(entry)-allowed:
            raise ValueError('Run sample requires sample_key/analysis and optional explicit quality definitions')
        key=_text(entry['sample_key'],'sample_key',256)
        if key in keys:raise ValueError('Run sample_key must be unique')
        if not isinstance(entry['analysis'],Mapping):raise ValueError('Sized sample report required')
        keys.add(key)
        control=entry.get('control_definition')
        if control is not None:
            if not isinstance(control,Mapping) or set(control)!={'schema_version','sample_type','markers'} or type(control['schema_version']) is not int or control['schema_version']!=1:
                raise ValueError('Explicit version1 control definition object required')
            if control['sample_type'] not in ('Positive Control','Negative Control','Sample') or not isinstance(control['markers'],Mapping) or len(control['markers'])>128:
                raise ValueError('Explicit bounded control type/marker definitions required')
            for name,recipe in control['markers'].items():
                _text(name,'control marker',128)
                if not isinstance(recipe,Mapping) or set(recipe)!={'expected_alleles','quality_sensor','is_y_marker'}:
                    raise ValueError('Explicit control marker recipe required')
                if type(recipe['quality_sensor']) is not bool or type(recipe['is_y_marker']) is not bool:raise ValueError('Explicit control booleans required')
                _labels(recipe['expected_alleles'],'expected_alleles')
        marker_quality=entry.get('marker_quality_definition')
        if marker_quality is not None:
            from .marker_quality_workflow import _definition
            marker_quality=_definition(marker_quality)
        normalized['samples'].append({'sample_key':key,'analysis':compact_analysis(entry['analysis']),
            'marker_quality_definition':deepcopy(marker_quality),'control_definition':deepcopy(control)})
    _canonical(normalized,MAX_RUN_INPUT_BYTES)
    return normalized


def _calculate(inputs):
    samples=[{k:r[k] for k in ('sample_key','analysis')} for r in inputs['samples']]
    outputs=run_str_batch_workflow(samples,inputs['ladders'],inputs['kit'],
        ladder_quality_definition=inputs['ladder_quality_definition'],missing_offset_policy=inputs['missing_offset_policy'])
    for entry,output in zip(inputs['samples'],outputs):
        report=output['report']
        if entry['control_definition'] is not None:
            from .control_workflow import attach_control_result
            report=attach_control_result(report,entry['control_definition'])
        if entry['marker_quality_definition'] is not None:
            from .marker_quality_workflow import attach_marker_quality
            report=attach_marker_quality(report,entry['marker_quality_definition'])
        output['report']=report
    return outputs


def _persist(store,run_id,revision_id,parent,inputs,calculated,actor,reason):
    previous={r['sample_key']:r['analysis_id'] for r in parent['outputs']} if parent else {}
    inputs_sha=_digest(_canonical(inputs,MAX_RUN_INPUT_BYTES));outputs=[]
    records=store.list_samples()
    for output in calculated:
        key=output['sample_key'];report=output['report'];source=report['source_sha256']
        report['settings']['project_run']={'run_id':run_id,'revision_id':revision_id,
            'parent_revision_id':parent['revision_id'] if parent else None,'sample_key':key,'inputs_sha256':inputs_sha}
        report['provenance']['project_run_reanalysis']={
            'policy':'new_immutable_results_for_all_registered_samples','previous_analysis_id':previous.get(key),
            'manual_reviews':'retained_on_previous_analysis; not_carried_forward',
            'sizing_repeated':False,'source_kind':'caller_supplied_compact_sized_observations'}
        metadata={'kind':'run-dependent sample','run_id':run_id,'sample_key':key}
        sample_id=next((r['sample_id'] for r in records if r['source_sha256']==source and r['metadata']==metadata),None)
        if sample_id is None:
            sample_id=store.add_sample(source,metadata);records.append({'sample_id':sample_id,'source_sha256':source,'metadata':metadata})
        analysis_id=store.add_analysis(sample_id,source,{'name':'OpenTrace Run','version':report['analysis_version']},
            report['settings'],report,report['status'])
        outputs.append({'sample_key':key,'analysis_id':analysis_id,'previous_analysis_id':previous.get(key)})
    identifier=store.append_run_revision(run_id,inputs,outputs,parent_revision_id=parent['revision_id'] if parent else None,
        actor=actor,reason=reason,revision_id=revision_id)
    # Bound the complete portable project before the atomic transaction commits.
    store.export_json()
    return {'run_id':run_id,'revision_id':identifier,'outputs':outputs}


def create_project_run(store,name,inputs,*,actor,reason):
    _text(name,'run name');_text(actor,'actor',256);_text(reason,'reason',4096)
    inputs=prepare_run_inputs(inputs);calculated=_calculate(inputs)
    run_id=str(uuid4());revision_id=str(uuid4())
    with store.atomic():
        store.add_run(name,run_id=run_id)
        return _persist(store,run_id,revision_id,None,inputs,calculated,actor,reason)


def revise_project_run(store,run_id,parent_revision_id,inputs,*,actor,reason):
    _text(actor,'actor',256);_text(reason,'reason',4096);store.get_run(run_id)
    history=store.list_run_revisions(run_id)
    if not history or history[-1]['revision_id']!=parent_revision_id:raise ValueError('Run parent revision is no longer current')
    parent=history[-1];inputs=prepare_run_inputs(inputs);calculated=_calculate(inputs)
    with store.atomic():
        return _persist(store,run_id,str(uuid4()),parent,inputs,calculated,actor,reason)


def change_run_ladder(store,run_id,parent_revision_id,ladder_id,*,analysis=None,remove=False,actor,reason):
    if type(remove) is not bool:raise ValueError('Explicit remove boolean required')
    parent=store.get_run_revision(parent_revision_id)
    if parent['run_id']!=run_id:raise ValueError('Run revision belongs to another run')
    inputs=deepcopy(parent['inputs']);_text(ladder_id,'ladder_id',256)
    matches=[i for i,r in enumerate(inputs['ladders']) if r['ladder_id'].lower()==ladder_id.lower()]
    if len(matches)!=1:raise ValueError('Select one existing run ladder')
    if remove:
        if analysis is not None:raise ValueError('Removal must not supply replacement analysis')
        inputs['ladders'].pop(matches[0])
    else:
        if not isinstance(analysis,Mapping):raise ValueError('Replacement sized analysis required')
        inputs['ladders'][matches[0]]['analysis']=compact_analysis(analysis)
    return revise_project_run(store,run_id,parent_revision_id,inputs,actor=actor,reason=reason)
