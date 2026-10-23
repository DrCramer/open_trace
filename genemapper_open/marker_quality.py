"""Explicit autosomal/AMEL/Y sample GQ subset; not a complete original processor.

Equations/order: research/IDX16_PQV.md, y.b(Sample), d helpers. Caller supplies
all thresholds, weights, artifact states and initial confidences. No defaults
or manufacturer data. Ladder/plugin/override routes are excluded here.
An accepted empty Y result reports its undefined original mean as None.
"""
from collections.abc import Mapping
import math
from .quality_control import _number,_bool


WEIGHTS=('allele_number','absolute_height','high_height','spike','ratio','broad','offscale','overlap','binning')
THRESHOLDS=('max_alleles','homozygous_height','heterozygous_height','high_height','ratio','width_bp')


def _score_marker(calls,thresholds,weights,flags,*,sample_sq,marker_kind,control_mismatch,amel_calls):
    if marker_kind not in ('autosomal','amel','y'):
        raise ValueError('Explicit autosomal, amel or y marker kind required')
    if amel_calls is not None:
        if not isinstance(amel_calls,(list,tuple)) or len(amel_calls)>128 or any(
                not isinstance(label,str) or not 1<=len(label)<=128 or not label.isascii() for label in amel_calls):
            raise ValueError('Explicit bounded ASCII AMEL calls or None required')
    _bool(control_mismatch,'control_mismatch')
    sq=_number(sample_sq,'sample_sq',0,1)
    if not isinstance(weights,Mapping) or set(weights)!=set(WEIGHTS):raise ValueError('All nine explicit quality weights required')
    weights={key:_number(weights[key],key,0,1) for key in WEIGHTS}
    if not isinstance(thresholds,Mapping) or set(thresholds)!=set(THRESHOLDS):raise ValueError('All six explicit thresholds required')
    maximum=thresholds['max_alleles']
    if type(maximum) is not int or not 1<=maximum<=128:raise ValueError('max_alleles must be1..128')
    thresholds={key:_number(value,key,0,1e12) for key,value in thresholds.items()}
    if thresholds['ratio']>1:raise ValueError('ratio threshold must not exceed1')
    if not isinstance(flags,Mapping) or set(flags)!= {'spike','offscale','overlap','binning'}:
        raise ValueError('Explicit spike/offscale/overlap/binning flags required')
    flags={key:_bool(value,key) for key,value in flags.items()}
    if not isinstance(calls,(list,tuple)) or len(calls)>4096:raise ValueError('At most4096 explicit calls required')
    prepared=[]
    for call in calls:
        if not isinstance(call,Mapping):raise ValueError('Call object required')
        label=call.get('label')
        if not isinstance(label,str) or not 1<=len(label)<=128 or not label.isascii():raise ValueError('Bounded ASCII label required')
        prepared.append({'label':label,'height':_number(call.get('height'),'height',0,1e12),
            'fwhm_bp':_number(call.get('fwhm_bp'),'fwhm_bp',0,1e6),
            'offscale':_bool(call.get('offscale'),'call.offscale'),
            'confidence':_number(call.get('confidence'),'initial confidence',0,1)})
    count=len(prepared)
    result={'status':'evaluated_subset','gq':1.0,'metrics':{key:-2 for key in WEIGHTS},'events':[],
        'initial_confidence_mean':None,'confidence_aggregate':None,'call_confidence_before_sq':[],
        'call_confidence_after_sq':[],'sample_sq':sq,'external_validation':False,
        'confidence_is_probability':False,'thresholds':thresholds,'weights':weights,
        'scope':'sample_marker_without_plugins_overrides_or_pullup_processor','marker_kind':marker_kind}
    metrics,events=result['metrics'],result['events']
    if not count and marker_kind!='y':
        result.update(gq=0.0,confidence_aggregate=0.0)
        metrics['allele_number']=1
        events.append({'criterion':'no_calls_non_y','forced_gq':0.0})
        return result
    initial=math.fsum(call['confidence'] for call in prepared)/count if count else None
    aggregate=initial
    result['initial_confidence_mean']=initial

    def penalize(criterion,reason=None):
        metrics[criterion]=1
        before=result['gq'];result['gq']*=1-weights[criterion]
        events.append({'criterion':criterion,'reason':reason or criterion,'factor':1-weights[criterion],
                       'gq_before':before,'gq_after':result['gq']})

    metrics['allele_number']=0
    count_valid=count<=maximum
    count_reason='called_count_above_maximum'
    if marker_kind=='y':
        if amel_calls is None or any(label.lower()=='y' for label in amel_calls):
            count_valid=1<=count<=maximum
            count_reason='y_count_without_amel_x_only'
        elif any(label.lower()=='x' for label in amel_calls):
            count_valid=count==0
            count_reason='y_calls_with_amel_x_only'
        else:
            count_valid=False
            count_reason='amel_has_neither_x_nor_y'
        result['amel_context']=None if amel_calls is None else list(amel_calls)
        if count==0 and count_valid:
            events.append({'criterion':'empty_y_confidence_mean','value':None,
                           'original_empty_mean_would_be_nan':True})
    if not count_valid:
        penalize('allele_number',count_reason)
        aggregate=0.0
        events.append({'criterion':'allele_number_confidence_aggregate','forced_value':0.0})
    if marker_kind=='amel' and not any(call['label'].lower()=='x' for call in prepared):
        # This is a separate multiplication even if the count already failed.
        penalize('allele_number','amel_without_x')
    heights=[call['height'] for call in prepared]
    absolute=thresholds['homozygous_height' if count==1 else 'heterozygous_height']
    metrics['absolute_height']=0 if count else -2
    if count and min(heights)<absolute:penalize('absolute_height')
    metrics['high_height']=0 if count else -2
    non_offscale=[call['height'] for call in prepared if not call['offscale']]
    if not non_offscale:metrics['high_height']=-2
    elif max(non_offscale)>thresholds['high_height']:penalize('high_height')
    metrics['spike']=int(flags['spike'])
    if flags['spike']:penalize('spike')
    metrics['ratio']=0 if count>=2 else -2
    ratio=min(heights)/max(heights) if count and max(heights)>0 else None
    result['observed_height_ratio']=ratio
    if count>=2 and ratio is not None and ratio<thresholds['ratio']:penalize('ratio')
    metrics['broad']=0 if count else -2
    if count and max(call['fwhm_bp'] for call in prepared)>thresholds['width_bp']:penalize('broad')
    metrics['offscale']=int(flags['offscale'] or any(call['offscale'] for call in prepared))
    if metrics['offscale']==1:penalize('offscale')
    metrics['overlap']=int(flags['overlap'])
    if flags['overlap']:penalize('overlap')
    metrics['binning']=int(flags['binning'])
    if flags['binning']:penalize('binning')
    if control_mismatch:
        result['gq']=0.0;events.append({'criterion':'positive_control_mismatch','forced_gq':0.0})
    result['confidence_aggregate']=aggregate
    before=aggregate*result['gq'] if aggregate is not None else None
    result['call_confidence_before_sq']=[before]*count
    result['call_confidence_after_sq']=[before*sq]*count if before is not None else []
    return result


def score_ordinary_marker(calls,thresholds,weights,flags,*,sample_sq,marker_kind,control_mismatch):
    if marker_kind not in ('autosomal','amel'):
        raise ValueError('Use the explicit Y API for y markers')
    return _score_marker(calls,thresholds,weights,flags,sample_sq=sample_sq,marker_kind=marker_kind,
                         control_mismatch=control_mismatch,amel_calls=None)


def score_y_marker(calls,thresholds,weights,flags,*,sample_sq,amel_calls,control_mismatch):
    """Non-ladder Y branch with explicit AMEL absence versus an empty result."""
    return _score_marker(calls,thresholds,weights,flags,sample_sq=sample_sq,marker_kind='y',
                         control_mismatch=control_mismatch,amel_calls=amel_calls)


def score_negative_marker(confidences, *, zero_peak_value, spike_metric, spike_weight, sample_sq):
    """Separate negative branch: preserve each initial confidence, then apply SQ.

    The original branch changes GQ/CC but bypasses the ordinary mean*GQ call
    assignment. Exact zero-peak0 fails; only spike state1 invokes its weight.
    Inputs are fresh observations, not stale original project metrics.
    """
    zero = _number(zero_peak_value, 'QC_ZERO_PEAK', -3, 1e6)
    spike = _number(spike_metric, 'spike_metric', -2, 1)
    if spike not in (-2, -1, 0, 1):
        raise ValueError('Explicit spike metric state -2/-1/0/1 required')
    weight = _number(spike_weight, 'spike weight', 0, 1)
    sq = _number(sample_sq, 'sample_sq', 0, 1)
    if not isinstance(confidences, (list, tuple)) or len(confidences) > 4096:
        raise ValueError('At most4096 initial call confidences required')
    initial = [_number(v, 'initial confidence', 0, 1) for v in confidences]
    gq = 0.0 if zero == 0 else 1.0
    events = [{'criterion': 'negative_zero_peak', 'value': zero, 'gq_after': gq}]
    if spike == 1:
        before = gq
        gq *= 1 - weight
        events.append({'criterion': 'spike', 'factor': 1 - weight,
                       'gq_before': before, 'gq_after': gq})
    return {'status': 'evaluated_subset', 'scope': 'fresh_negative_control_zero_peak_and_spike',
            'gq': gq, 'cc': int(zero == 0), 'metrics': {'zero_peak': zero, 'spike': spike},
            'events': events, 'sample_sq': sq, 'spike_weight': weight,
            'initial_confidence_mean': None, 'confidence_aggregate': None,
            'ordinary_confidence_assignment_applied': False,
            'call_confidence_before_sq': initial,
            'call_confidence_after_sq': [v * sq for v in initial],
            'external_validation': False, 'confidence_is_probability': False}
