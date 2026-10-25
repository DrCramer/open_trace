"""Independent explicit-kit, single-ladder research workflow.

Public analysis reports are supplied by the caller; their accepted state is
not an external attestation. analyze_str_files binds newly computed reports
to exact input bytes. No manufacturer data or original runtime is loaded.
"""
from collections.abc import Mapping
from .fastcopy import fastcopy
import hashlib
import json
import math
import re
from .kit_definition import load_kit_definition
from .ladder import match_ladder
from .allele_calling import call_sample
from . import __version__
from .calibration import build_calibration
from .quality_control import dimensions_in_bp


def digest(value):
    return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':'),
                                    ensure_ascii=False,allow_nan=False).encode('utf-8')).hexdigest()


def compact_analysis(report):
    """Remove raw arrays/directory before sending a public report to this API."""
    return {**{key:fastcopy(report[key]) for key in ('source_sha256','analysis_version',
            'peak_settings','sizing','standard_definition_context','pullup_detection') if key in report},
        'channels':[{key:fastcopy(channel[key]) for key in ('tag','group','peaks') if key in channel}
                    for channel in report.get('channels',[])]}


def _analysis(report, name, channels):
    if not isinstance(report,Mapping):
        raise ValueError(name+': analysis report required')
    if not isinstance(report.get('source_sha256'),str) or not re.fullmatch('[0-9a-f]{64}',report['source_sha256']):
        raise ValueError(name+': input SHA-256 required')
    if not isinstance(report.get('analysis_version'),str) or len(report['analysis_version']) > 128:
        raise ValueError(name+': bounded analysis version required')
    sizing, settings = report.get('sizing'), report.get('peak_settings')
    if not isinstance(sizing,Mapping) or not isinstance(settings,Mapping):
        raise ValueError(name+': sizing and peak_settings required')
    signature = {'peak_settings':dict(settings),
        'sizing':{key:fastcopy(sizing.get(key)) for key in ('mode','reference_channel','expected_bp',
            'analysis_engine','calibration_model','max_residual_bp','settings')}}
    accepted = (sizing.get('status') == 'accepted' and sizing.get('mode') == 'auto'
        and sizing.get('calibration_applied') is True and len(sizing.get('anchors',[])) >= 3
        and sizing.get('analysis_engine') == 'morphology_ratio'
        and settings.get('analysis_engine') == 'morphology_ratio')
    curve=build_calibration(sizing['anchors'],sizing['calibration_model']) if accepted else None
    rows = report.get('channels')
    if not isinstance(rows,list) or len(rows)>128:
        raise ValueError(name+': bounded channel list required')
    selected = settings.get('selected_data_group')
    if selected not in ('raw','analyzed'):
        raise ValueError(name+': selected raw/analyzed group required')
    found, peaks, skipped = set(), [], 0
    spike_unavailable=[];pullup_unavailable=[]
    pullup_definition=settings.get('pullup_definition')
    if pullup_definition is not None:
        from .pullup import load_pullup_definition
        pullup_definition=load_pullup_definition(pullup_definition)
        signature['peak_settings']['pullup_definition']=pullup_definition
    reference = sizing.get('reference_channel')
    if reference in channels:
        raise ValueError('Kit marker channel must differ from the size-standard channel')
    all_tags = set()
    for channel in rows:
        if not isinstance(channel,Mapping) or not isinstance(channel.get('tag'),str):
            raise ValueError(name+': invalid channel')
        tag = channel['tag']
        if tag in all_tags:
            raise ValueError(name+': duplicate channel')
        all_tags.add(tag)
        if tag not in channels or channel.get('group') != selected:
            continue
        found.add(tag)
        candidates = channel.get('peaks')
        if not isinstance(candidates,list) or len(candidates)>4096:
            raise ValueError(name+': at most4096 peaks per channel')
        for index,peak in enumerate(candidates):
            if not isinstance(peak,Mapping):
                raise ValueError(name+': peak object required')
            restored=(peak.get('true_peak_restoration') or {}).get('status')=='restored'
            if peak.get('size_bp') is None or (restored and type(peak.get('size_bp')) in (int,float) and peak['size_bp']<=0):
                skipped += 1
                continue
            for key in ('scan','size_bp','height'):
                value=peak.get(key)
                if type(value) not in (int,float) or not math.isfinite(value):
                    raise ValueError(name+': finite numeric peak coordinates/height required')
            if not (0<=peak['scan']<=100_000_000 and 0<peak['size_bp']<=1_000_000 and 0<peak['height']<=1e12):
                raise ValueError(name+': peak outside supported bounds')
            if 'pullup' in peak and type(peak['pullup']) is not bool:
                raise ValueError(name+': boolean pull-up flag required')
            record = {key:fastcopy(peak[key]) for key in ('scan','size_bp','height','flags','spike','pullup') if key in peak}
            for key in ('begin','end','fwhm_scan','area','area_bp','offscaled','spike_features','spike_classification','true_peak_restoration',
                        'pullup_classification','pullup_ratio','pullup_ratio_threshold'):
                if key in peak:record[key]=fastcopy(peak[key])
            if settings.get('spike_definition') is not None and (peak.get('spike_classification') or {}).get('status')!='evaluated_subset':
                spike_unavailable.append(f'{tag}:{index}')
            if pullup_definition is not None and pullup_definition['detector'] is not None:
                if (peak.get('pullup_classification') or {}).get('status') not in ('not_targeted','evaluated_explicit_passes'):
                    pullup_unavailable.append(f'{tag}:{index}')
            if restored and peak.get('area',0)<0:
                record['morphology_bp']={'status':'unavailable','reason':'restored_negative_area_sentinel',
                    'area':peak['area'],'area_bp':peak.get('area_bp')}
            elif curve and all(key in peak for key in ('begin','end','fwhm_scan','area')):
                record['morphology_bp']=dimensions_in_bp({'begin_scan':peak['begin'],'end_scan':peak['end'],
                    'fwhm_scan':peak['fwhm_scan'],'area':peak['area']},curve)
                if restored:
                    record['morphology_bp'].update(area_bp=peak['area_bp'],area_source='true_peak_restored_unrounded_map_span')
            else:
                record['morphology_bp']={'status':'unavailable','reason':'missing_morphology_or_calibration'}
            record.update(channel=tag,id=f'{tag}:{index}')
            peaks.append(record)
    if len(peaks)>4096:
        raise ValueError(name+': at most4096 total sized kit-channel peaks')
    return {'accepted':accepted,'signature':signature,'peaks':peaks,'missing_channels':sorted(channels-found),
            'unsized_peaks':skipped,'source_sha256':report['source_sha256'],'spike_unavailable':spike_unavailable,
            'pullup_unavailable':pullup_unavailable,
            'analysis_version':report['analysis_version'],'sizing_status':sizing.get('status'),
            'standard_definition_context':fastcopy(report.get('standard_definition_context'))}


def _ladder_stages(ladder_analysis,ladder,definition,quality_definition,missing_offset_policy,adjustments,*,run_offset_state=None):
    """Match/call one ladder once; retain pre-gate offsets for run aggregation."""
    options={'run_offset_state':run_offset_state} if run_offset_state is not None else {}
    matching=match_ladder(ladder['peaks'],definition,
        size_standard_max_bp=max(ladder_analysis['sizing']['expected_bp']),sizing_accepted=True,**options)
    policy=quality_definition['calling_parameters_source'] if quality_definition else 'ladder_matching_calls'
    if policy=='ladder_matching_calls':
        from .ladder_calling import ladder_call_observations
        observations=ladder_call_observations(ladder['peaks'],definition,matching)
    else:
        from .ladder_observation_binding import bind
        observations=bind(ladder['peaks'],definition,matching,
            call_sample(ladder['peaks'],definition,matching,sizing_accepted=True,
                missing_offset_policy=missing_offset_policy))
    provenance={'ladder_call_observation_source':policy,'ladder_sample_filters_applied':policy=='kit_sample_parameters'}
    quality={}
    if adjustments is not None:
        from .spike_adjustment import adjust_ladder_spikes,apply_peak_states
        observations=adjust_ladder_spikes(ladder['peaks'],observations,adjustments)
        quality['spike_adjustments']={'ladder':observations['spike_adjustments'],'ladder_call_observations':observations}
        ladder['peaks']=apply_peak_states(ladder['peaks'],observations['spike_adjustments'])
        provenance.update(artifact_processing_scope='initial_spike_and_current_boolean_adjustment_subset',
            downstream_spike_adjustments_applied=True)
    quality['ladder_calling']=observations
    pullup_definition=ladder['signature']['peak_settings'].get('pullup_definition')
    if pullup_definition is not None:
        from .pullup_quality import evaluate_pullup_quality
        quality['ladder_pullup']=evaluate_pullup_quality({'peaks':ladder['peaks'],'allele_calling':observations},pullup_definition)
    effective=matching
    if quality_definition is not None:
        from .ladder_quality_workflow import gate_ladder_offsets
        effective,quality['ladder_gq']=gate_ladder_offsets(ladder['peaks'],definition,matching,quality_definition,
            sizing=ladder_analysis['sizing'],missing_offset_policy=missing_offset_policy,call_observations=observations)
        provenance['ladder_quality_applied']=True
    return matching,effective,quality,provenance


def _sample_stage(result,sample,definition,offsets,adjustments):
    called=call_sample(sample['peaks'],definition,offsets,sizing_accepted=True,
        missing_offset_policy=result['settings']['missing_offset_policy'],
        source_metadata={'sha256':sample['source_sha256']},spike_adjustments=adjustments)
    if adjustments is not None:
        from .spike_adjustment import apply_peak_states
        summary=called.get('spike_adjustments')
        result.setdefault('quality',{}).setdefault('spike_adjustments',{})['sample']=summary or {
            'status':'unavailable','reason':'sample_call_observations_rejected'}
        if summary is not None:sample['peaks']=apply_peak_states(sample['peaks'],summary)
    result['allele_calling']=called;result['status']=called['status'];result['reasons']=list(called['reasons'])
    result['peaks']=sample['peaks']
    for marker in called['markers']:
        for call in marker['calls']:
            source=sample['peaks'][call['peak_index']]
            identifier=hashlib.sha256((marker['name']+'\0'+source['id']).encode()).hexdigest()[:24]
            result['calls'].append({**fastcopy(call),'id':identifier,'marker':marker['name'],
                                    'channel':marker['channel'],'source_peak_id':source['id']})
    pullup_definition=sample['signature']['peak_settings'].get('pullup_definition')
    if pullup_definition is not None:
        from .pullup_quality import evaluate_pullup_quality
        result.setdefault('quality',{})['pullup']=evaluate_pullup_quality(result,pullup_definition)
        result['settings']['pullup_quality_definition_sha256']=result['quality']['pullup']['definition_sha256']


def _pullup_provenance(sample):
    definition=sample['signature']['peak_settings'].get('pullup_definition')
    if definition is None:return {}
    return {'automatic_pullup_applied':definition['detector'] is not None,
        'experimental_pullup_routing_applied':definition['detector'] is not None,
        'original_automatic_pullup_route_confirmed':False}


def run_str_workflow(sample_analysis, ladder_analysis, kit, *, missing_offset_policy='reject',ladder_quality_definition=None):
    """Return tentative calls only, with explicit provenance and stage failures."""
    definition = load_kit_definition(kit)
    if missing_offset_policy not in ('reject','forward_fill'):
        raise ValueError('missing_offset_policy must be reject or forward_fill')
    channels={marker['channel'] for marker in definition['markers']}
    sample = _analysis(sample_analysis,'sample',channels)
    ladder = _analysis(ladder_analysis,'ladder',channels)
    settings = {'analysis':sample['signature'],'missing_offset_policy':missing_offset_policy}
    if ladder_quality_definition is not None:
        from .ladder_quality_workflow import load_ladder_quality_definition
        ladder_quality_definition=load_ladder_quality_definition(ladder_quality_definition)
        settings['ladder_quality_definition_sha256']=digest(ladder_quality_definition)
    result = {'schema_version':1,'source_sha256':sample['source_sha256'],
        'analysis_version':'OpenTrace/'+__version__,'status':'rejected','reasons':[],
        'settings':settings,'kit_identity':{'name':definition['name'],'sha256':digest(definition)},
        'sizing':{key:fastcopy(sample_analysis['sizing'].get(key)) for key in
                  ('status','mode','anchors','calibration_model','reference_channel','base_quality','post_quality','expected_bp')},
        'sample_identity':{key:sample[key] for key in ('source_sha256','analysis_version','sizing_status','standard_definition_context')},
        'ladder_identity':{key:ladder[key] for key in ('source_sha256','analysis_version','sizing_status','standard_definition_context')},
        'ladder':None,'allele_calling':None,'calls':[],'peaks':[], 'research_only':True,
        'provenance':{'sample_settings_sha256':digest(sample['signature']),
            'ladder_settings_sha256':digest(ladder['signature']), 'external_validation':False,
            'original_runtime_validated':False,'input_reports_are_caller_supplied':True,
            'artifact_detectors_applied':False,'full_original_pqv_applied':False}}
    result['provenance']['ladder_quality_applied']=False
    if sample['signature'] != ladder['signature']:
        result['reasons'].append('Sample and ladder require identical explicit analysis/standard settings')
    if sample['analysis_version'] != ladder['analysis_version']:
        result['reasons'].append('Sample and ladder require the same analysis version')
    for name,item in (('sample',sample),('ladder',ladder)):
        if not item['accepted']:
            result['reasons'].append(name+': accepted automatic morphology sizing is required')
        if item['missing_channels']:
            result['reasons'].append(name+': kit channels absent from selected data group: '+', '.join(item['missing_channels']))
        if item['spike_unavailable']:
            result['reasons'].append(name+': requested initial spike classification unavailable for sized kit-channel peaks')
        if item['pullup_unavailable']:
            result['reasons'].append(name+': requested explicit pull-up processing unavailable for sized kit-channel peaks')
    if result['reasons']:
        return result
    result['provenance']['artifact_detectors_applied']=sample['signature']['peak_settings'].get('spike_definition') is not None
    if result['provenance']['artifact_detectors_applied']:
        result['provenance']['artifact_processing_scope']='initial_non_reference_spike_subset_without_downstream_adjustments'
        if sample['signature']['peak_settings']['spike_definition'].get('schema_version',0)>=3:
            result['provenance']['morphology_scope']='companion_jag_and_ideal_gaussian_fit_only'
            result['provenance']['true_peak_restoration_applied']=False
            if sample['signature']['peak_settings']['spike_definition']['schema_version']==4:
                result['provenance'].update(morphology_scope='side_collection_true_peak_and_fresh_restoration',
                    true_peak_restoration_applied=any(bool(p.get('true_peak_restoration',{}).get('writes')) for p in sample['peaks']))
        result['provenance']['automatic_pullup_applied']=False
    result['provenance'].update(_pullup_provenance(sample))
    result['provenance']['artifact_detectors_applied']|=result['provenance'].get('experimental_pullup_routing_applied',False)
    spike_definition=sample['signature']['peak_settings'].get('spike_definition')
    adjustments=None
    if spike_definition is not None:
        from .spike import load_spike_definition
        spike_definition=load_spike_definition(spike_definition)
        adjustments=spike_definition.get('adjustments')
    matching,ladder_result,quality,provenance=_ladder_stages(ladder_analysis,ladder,definition,
        ladder_quality_definition,missing_offset_policy,adjustments)
    result['quality']=quality;result['provenance'].update(provenance);result['ladder']=ladder_result
    ladder_observations=quality['ladder_calling']
    _sample_stage(result,sample,definition,ladder_result,adjustments)
    called=result['allele_calling']
    # Bind all machine observations; review is subsequently a separate overlay.
    observations={'ladder':ladder_result,'allele_calling':called}
    if ladder_observations is not None:observations['ladder_calling']=ladder_observations
    result['provenance']['machine_observations_sha256']=digest(observations)
    return result


def analyze_str_files(sample_bytes,ladder_bytes,kit,*,settings,missing_offset_policy='reject',ladder_quality_definition=None):
    from .server import analyze
    if not isinstance(settings,dict):
        raise ValueError('Explicit analysis settings object required')
    sample=analyze(sample_bytes,'sample.fsa',**settings)
    ladder=analyze(ladder_bytes,'ladder.fsa',**settings)
    result=run_str_workflow(sample,ladder,kit,missing_offset_policy=missing_offset_policy,
                           ladder_quality_definition=ladder_quality_definition)
    result['provenance']['input_reports_are_caller_supplied']=False
    return result
