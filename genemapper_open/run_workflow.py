"""Explicit fresh run: analyze ladders, aggregate admitted offsets, call sample.

Each ladder is processed once before sample calling. Fresh subset AL is used
as its numerical admission code; unavailable quality never becomes original
NA/override codes. This is not a complete original Run/PQV lifecycle.
"""
from collections.abc import Mapping
from .fastcopy import fastcopy
import hashlib
import math
import pickle
import threading
from collections import OrderedDict
from . import __version__
from .kit_definition import load_kit_definition
from .str_workflow import _analysis,_ladder_stages,_sample_stage,_pullup_provenance,digest
from .ladder_quality_workflow import load_ladder_quality_definition


def _ladder_entries(ladders,field):
    if not isinstance(ladders,(list,tuple)) or not 1<=len(ladders)<=128:
        raise ValueError('Explicit1..128 run ladders required')
    for entry in ladders:
        if not isinstance(entry,Mapping) or set(entry)!={'ladder_id',field}:
            raise ValueError('Each run ladder requires ladder_id and '+field)
        identifier=entry['ladder_id']
        if (not isinstance(identifier,str) or not 1<=len(identifier)<=256
                or not identifier.isascii() or any(ord(c)<32 or ord(c)==127 for c in identifier)):
            raise ValueError('Bounded printable ASCII run ladder_id required')
    return ladders


POLICIES=('reject','forward_fill','reference_bins')


def _ladder_policy(policy):
    # Reference bins only replace offsets in sample calling; ladders are judged as with forward fill.
    return 'forward_fill' if policy=='reference_bins' else policy


def _base_run_report(sample_analysis,sample,definition,quality_definition,missing_offset_policy):
    return {'schema_version':1,'source_sha256':sample['source_sha256'],'analysis_version':'OpenTrace/'+__version__,
        'status':'rejected','reasons':[],'calls':[],'peaks':[],'allele_calling':None,'ladder':None,
        'research_only':True,'kit_identity':{'name':definition['name'],'sha256':digest(definition)},
        'sample_identity':{key:sample[key] for key in ('source_sha256','analysis_version','sizing_status','standard_definition_context')},
        'ladder_identity':{'scope':'explicit_run','ladders':[]},
        'settings':{'analysis':sample['signature'],'missing_offset_policy':missing_offset_policy,
            'ladder_quality_definition_sha256':digest(quality_definition)},
        'sizing':{key:fastcopy(sample_analysis['sizing'].get(key)) for key in
            ('status','mode','anchors','calibration_model','reference_channel','base_quality','post_quality','expected_bp')},
        'quality':{'run_ladders':[]},
        'provenance':{'scope':'fresh_explicit_run','input_reports_are_caller_supplied':True,
            'sample_settings_sha256':digest(sample['signature']),'external_validation':False,
            'original_runtime_validated':False,'full_original_pqv_applied':False,
            'run_quality_code_source':'fresh_ladder_al_subset','ladder_quality_applied':True}}


def _sample_failures(sample):
    reasons=[]
    if not sample['accepted']:reasons.append('sample: accepted automatic morphology sizing is required')
    if sample['missing_channels']:reasons.append('sample: kit channels absent from selected data group')
    if sample['spike_unavailable']:reasons.append('sample: requested spike classification unavailable')
    if sample['pullup_unavailable']:reasons.append('sample: requested pull-up processing unavailable')
    return reasons


def _artifact_options(sample):
    spike_definition=sample['signature']['peak_settings'].get('spike_definition')
    adjustments=None
    if spike_definition is not None:
        from .spike import load_spike_definition
        adjustments=load_spike_definition(spike_definition).get('adjustments')
    return spike_definition,adjustments


def _marker_quality_codes(quality):
    """Per-marker admission codes: a marker's offsets are admitted when its own GQ passes and the ladder SQ passes.

    Markers absent from the result are not admitted (None)."""
    gq=quality.get('ladder_gq') or {}
    sq=(gq.get('sq_category') or {}).get('code')
    codes={}
    for row in gq.get('markers',[]):
        if row.get('status')!='evaluated_subset':continue
        passed=(row.get('category') or {}).get('code')==0 and sq==0
        codes[row['marker']]=0 if passed else 2
    return codes


def _ladder_cohort(sample,ladders,definition,quality_definition,missing_offset_policy,adjustments,*,marker_gate=False):
    from .run_offsets import aggregate_run_offsets,remove_run_ladder,upsert_run_offsets
    channels={m['channel'] for m in definition['markers']}
    result={'ladder_identity':{'scope':'explicit_run','ladders':[]},'quality':{'run_ladders':[]}}
    entries=[];state=None
    for entry in ladders:
        identifier=entry['ladder_id'];analysis=entry['analysis'];ladder=_analysis(analysis,'ladder '+identifier,channels)
        identity={key:ladder[key] for key in ('source_sha256','analysis_version','sizing_status','standard_definition_context')}
        result['ladder_identity']['ladders'].append({'ladder_id':identifier,**identity})
        reasons=[]
        if sample['signature']!=ladder['signature']:reasons.append('Analysis/standard settings differ from sample')
        if sample['analysis_version']!=ladder['analysis_version']:reasons.append('Analysis version differs from sample')
        if not ladder['accepted']:reasons.append('Accepted automatic morphology sizing required')
        if ladder['missing_channels']:reasons.append('Kit channels absent from selected data group')
        if ladder['spike_unavailable']:reasons.append('Requested spike classification unavailable')
        stage={'ladder_id':identifier,'identity':identity,'status':'rejected','reasons':reasons,
            'quality_code':None,'quality_code_source':'fresh_ladder_al_subset',
            'matching':{'status':'rejected','markers':{},'reasons':reasons},'effective':None,'quality':{},'provenance':{}}
        prior=remove_run_ladder(state,identifier)
        if not reasons:
            matching,effective,quality,provenance=_ladder_stages(analysis,ladder,definition,quality_definition,
                missing_offset_policy,adjustments,run_offset_state=prior)
            code=quality['ladder_gq']['al_subset_code']
            stage.update(status='evaluated_subset',matching=matching,effective=effective,
                quality=quality,provenance=provenance,quality_code=code)
        result['quality']['run_ladders'].append(stage)
        admitted=stage['quality_code']
        if marker_gate and stage['status']=='evaluated_subset':
            admitted=_marker_quality_codes(stage['quality'])
        entries.append({'ladder_id':identifier,'quality_code':admitted,'result':stage['matching']})
        state=upsert_run_offsets(prior,identifier,admitted,stage['matching'])
    offsets=(aggregate_run_offsets(entries,definition) if entries else {
        'status':'rejected','reasons':['No ladders remain in this run'],'markers':{},
        'diagnostics':{'method':'empty_explicit_run','external_validation':False}})
    result['ladder']=offsets
    return result


_COHORTS=OrderedDict();_COHORT_LOCK=threading.Lock();_COHORT_MAX=8


def _cached_cohort(sample,ladders,kit,ladder_quality_definition,missing_offset_policy,definition,quality_definition,adjustments,marker_gate=False):
    """Ladder processing does not depend on which sample of the run is called, so a run's samples share it.

    The cohort is read-only (``_finish_run`` copies every part it uses). The key covers everything the
    cohort is computed from; ladders are compared by content, not by name."""
    try:
        # pickle, not JSON: several times faster for the large ladder reports; a different key order only
        # costs a cache miss
        key=hashlib.sha256(pickle.dumps((sample['signature'],sample['analysis_version'],ladders,kit,
            ladder_quality_definition,missing_offset_policy,marker_gate),protocol=pickle.HIGHEST_PROTOCOL)).digest()
    except (TypeError,ValueError,pickle.PicklingError,AttributeError,RecursionError):
        key=None
    if key is not None:
        with _COHORT_LOCK:
            hit=_COHORTS.get(key)
            if hit is not None:_COHORTS.move_to_end(key);return hit
    cohort=_ladder_cohort(sample,ladders,definition,quality_definition,_ladder_policy(missing_offset_policy),adjustments,
        marker_gate=marker_gate)
    if key is not None:
        with _COHORT_LOCK:
            _COHORTS[key]=cohort
            while len(_COHORTS)>_COHORT_MAX:_COHORTS.popitem(last=False)
    return cohort


def _finish_run(result,sample,definition,cohort,adjustments,spike_definition,call_thresholds=None):
    if spike_definition is not None and spike_definition.get('schema_version',0)>=3:
        result['provenance'].update(morphology_scope='companion_jag_and_ideal_gaussian_fit_only',
            true_peak_restoration_applied=False)
        if spike_definition['schema_version']==4:
            result['provenance'].update(morphology_scope='side_collection_true_peak_and_fresh_restoration',
                true_peak_restoration_applied=any(bool(p.get('true_peak_restoration',{}).get('writes')) for p in sample['peaks']))
    result['ladder_identity']=fastcopy(cohort['ladder_identity'])
    result['quality']['run_ladders']=fastcopy(cohort['quality']['run_ladders'])
    offsets=result['ladder']=fastcopy(cohort['ladder'])
    result['provenance'].update(artifact_detectors_applied=spike_definition is not None,automatic_pullup_applied=False,
        downstream_spike_adjustments_applied=adjustments is not None)
    result['provenance'].update(_pullup_provenance(sample))
    result['provenance']['artifact_detectors_applied']|=result['provenance'].get('experimental_pullup_routing_applied',False)
    result['provenance']['run_inputs_sha256']=digest({'sample':sample['source_sha256'],
        'ladders':result['ladder_identity']['ladders'],'stages':result['quality']['run_ladders']})
    _sample_stage(result,sample,definition,offsets,adjustments,call_thresholds=call_thresholds)
    result['provenance']['machine_observations_sha256']=digest({'ladder':offsets,
        'allele_calling':result['allele_calling'],'run_ladders':result['quality']['run_ladders']})
    return result


def _call_thresholds(homozygous,heterozygous):
    """ID-X sample-call RFU floors (LPH): homozygous and heterozygous. Sample calls only, never ladders."""
    values={}
    for name,value in (('homozygous',homozygous),('heterozygous',heterozygous)):
        if not isinstance(value,(int,float)) or isinstance(value,bool) or not math.isfinite(value) or value<0:
            raise ValueError(name+'_min_height must be a finite RFU value >= 0')
        values[name]=float(value)
    return values


def run_str_run_workflow(sample_analysis,ladders,kit,*,ladder_quality_definition,missing_offset_policy='reject',marker_gate=False,homozygous_min_height=200.0,heterozygous_min_height=100.0):
    """Use an ordered explicit run cohort; duplicate filename means reanalysis."""
    definition=load_kit_definition(kit)
    quality_definition=load_ladder_quality_definition(ladder_quality_definition)
    if missing_offset_policy not in POLICIES:
        raise ValueError('missing_offset_policy must be reject, forward_fill or reference_bins')
    if ladders or missing_offset_policy!='reference_bins':_ladder_entries(ladders,'analysis')
    elif not isinstance(ladders,(list,tuple)):raise ValueError('Explicit run ladder list required')
    sample=_analysis(sample_analysis,'sample',{m['channel'] for m in definition['markers']})
    call_thresholds=_call_thresholds(homozygous_min_height,heterozygous_min_height)
    result=_base_run_report(sample_analysis,sample,definition,quality_definition,missing_offset_policy)
    result['settings']['call_thresholds']=call_thresholds
    result['reasons']=_sample_failures(sample)
    if result['reasons']:return result
    spike_definition,adjustments=_artifact_options(sample)
    cohort=_cached_cohort(sample,ladders,kit,ladder_quality_definition,missing_offset_policy,definition,quality_definition,adjustments,
        marker_gate=marker_gate)
    return _finish_run(result,sample,definition,cohort,adjustments,spike_definition,call_thresholds=call_thresholds)


def run_str_batch_workflow(samples,ladders,kit,*,ladder_quality_definition,missing_offset_policy='reject',marker_gate=False,homozygous_min_height=200.0,heterozygous_min_height=100.0):
    """Process an explicit sized cohort once, then every dependent sample once.

    The first sample supplies the common analysis-settings context. Its sizing
    may be rejected; that does not suppress valid sibling samples. Incompatible
    siblings retain rejected reports. Malformed inputs abort before persistence.
    """
    definition=load_kit_definition(kit);quality_definition=load_ladder_quality_definition(ladder_quality_definition)
    if missing_offset_policy not in POLICIES:raise ValueError('Invalid missing offset policy')
    if not isinstance(ladders,(list,tuple)):raise ValueError('Explicit run ladder list required')
    if ladders:_ladder_entries(ladders,'analysis')
    if not isinstance(samples,(list,tuple)) or len(samples)>128:raise ValueError('Explicit0..128 run samples required')
    if not samples:return []
    keys=set();prepared=[];channels={m['channel'] for m in definition['markers']}
    for entry in samples:
        if not isinstance(entry,Mapping) or set(entry)!={'sample_key','analysis'}:raise ValueError('Run sample needs sample_key/analysis')
        key=entry['sample_key']
        if not isinstance(key,str) or not key.strip() or len(key)>256 or key in keys:raise ValueError('Bounded unique sample_key required')
        keys.add(key);prepared.append(_analysis(entry['analysis'],'sample '+key,channels))
    reference=prepared[0];spike_definition,adjustments=_artifact_options(reference)
    call_thresholds=_call_thresholds(homozygous_min_height,heterozygous_min_height)
    cohort=_ladder_cohort(reference,ladders,definition,quality_definition,_ladder_policy(missing_offset_policy),adjustments,
        marker_gate=marker_gate)
    outputs=[]
    for entry,sample in zip(samples,prepared):
        result=_base_run_report(entry['analysis'],sample,definition,quality_definition,missing_offset_policy)
        result['reasons']=_sample_failures(sample)
        if sample['signature']!=reference['signature']:result['reasons'].append('Sample analysis/standard settings differ from run context')
        if sample['analysis_version']!=reference['analysis_version']:result['reasons'].append('Sample analysis version differs from run context')
        result['settings']['call_thresholds']=call_thresholds
        if not result['reasons']:_finish_run(result,sample,definition,cohort,adjustments,spike_definition,call_thresholds=call_thresholds)
        else:
            result['ladder_identity']=fastcopy(cohort['ladder_identity'])
            result['quality']['run_ladders']=fastcopy(cohort['quality']['run_ladders'])
            result['ladder']=fastcopy(cohort['ladder'])
            result['allele_calling']={'status':'rejected','reasons':list(result['reasons']),
                'markers':[{'name':m['name'],'channel':m['channel'],'status':'rejected',
                    'reasons':['Sample prerequisites unavailable'],'calls':[],'stutter_calls':[],'peak_review':[]}
                    for m in definition['markers']],
                'diagnostics':{'method':'skipped_due_to_sample_prerequisites','calling_executed':False}}
        result['provenance'].update(batch_sample_key=entry['sample_key'],
            batch_inputs_sha256=digest({'samples':samples,'ladders':ladders,'kit':definition,'quality_definition':quality_definition,
                'missing_offset_policy':missing_offset_policy}),reanalysis_scope='sized_observations_to_calls_and_requested_quality')
        outputs.append({'sample_key':entry['sample_key'],'report':result})
    return outputs


def analyze_str_run_files(sample_bytes,ladders,kit,*,settings,ladder_quality_definition,missing_offset_policy='reject'):
    from .server import analyze
    if not isinstance(settings,dict):raise ValueError('Explicit analysis settings object required')
    _ladder_entries(ladders,'data')
    sample=analyze(sample_bytes,'sample.fsa',**settings)
    reports=[{'ladder_id':r['ladder_id'],'analysis':analyze(r['data'],r['ladder_id'],**settings)} for r in ladders]
    result=run_str_run_workflow(sample,reports,kit,ladder_quality_definition=ladder_quality_definition,
        missing_offset_policy=missing_offset_policy)
    result['provenance']['input_reports_are_caller_supplied']=False
    return result
