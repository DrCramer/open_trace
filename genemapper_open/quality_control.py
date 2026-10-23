"""Explicit independent QC helpers from statically observed ID-X contracts.

No default laboratory thresholds, vendor runtime, or authentication is supplied.
These helpers are not a complete original PQV processor. They preserve numeric
quality/category distinctions and record skipped/unknown prerequisites.
"""
import math
from numbers import Real
from collections.abc import Mapping


def _number(value,name,low=-1e12,high=1e12):
    try:valid=isinstance(value,Real) and not isinstance(value,bool) and math.isfinite(value) and low<=value<=high
    except (ValueError,OverflowError):valid=False
    if not valid:raise ValueError(name+': finite bounded number required')
    return float(value)


def _bool(value,name):
    if type(value) is not bool:raise ValueError(name+': explicit boolean required')
    return value


def _metrics(values):
    if not isinstance(values,(list,tuple)) or len(values)>128:
        raise ValueError('At most128 explicit marker values required')
    return [None if value is None else _number(value,'metric',-3,1e6) for value in values]


def quality_category(score,*,good,bad):
    """HID boundary rule: good equality passes, bad equality checks."""
    good,bad=_number(good,'good',0,1),_number(bad,'bad',0,1)
    if bad>good:raise ValueError('bad must not exceed good')
    score=_number(score,'score',0,1)
    code=0 if score>=good else 2 if score<bad else 1
    return {'code':code,'state':('pass','check','fail')[code],'score':score,'good':good,'bad':bad}


def profile_pullup_category(marker_values,*,threshold):
    """All markers remain in denominator, including missing/NA metrics."""
    values=_metrics(marker_values);threshold=_number(threshold,'threshold',0,1)
    available=any(value is not None and value>=0 for value in values)
    count=sum(value is not None and value>0 for value in values)
    ratio=count/len(values) if values else None
    code=(-2 if not available else 2 if ratio>threshold else 1 if count else 0)
    return {'code':code,'denominator':len(values),'positive_markers':count,'fraction':ratio,
            'available':available,'threshold':threshold,'scope':'profile_pullup_only'}


def composite_control_category(marker_cc_values):
    values=_metrics(marker_cc_values)
    available=any(value in (0,1) for value in values)
    return {'code':2 if 1 in values else 0 if available else -2,
            'available':available,'failed_markers':sum(value==1 for value in values)}


def _labels(value,name):
    if value is None:return None
    if not isinstance(value,(list,tuple)) or len(value)>128:
        raise ValueError(name+': bounded explicit label list or None required')
    if any(not isinstance(label,str) or not 1<=len(label)<=128 or not label.isascii()
           or any(ord(c)<32 or ord(c)==127 for c in label) for label in value):
        raise ValueError(name+': bounded ASCII allele labels required; no Unicode equality parity claimed')
    return list(value)


def positive_control(called,expected,*,is_y_marker):
    """Original membership/count rule, including duplicate asymmetry; no trim.

    Missing expected is comparison-skipped, rather than a validated control.
    ASCII bounds avoid claiming Python casefold equals Java Unicode behavior.
    """
    called,expected=_labels(called,'called'),_labels(expected,'expected')
    _bool(is_y_marker,'is_y_marker')
    called=called or []
    no_call_non_y=not called and not is_y_marker
    performed=bool(expected)
    mismatch=performed and (len(called)!=len(expected)
        or any(not any(c.lower()==e.lower() for e in expected) for c in called))
    return {'cc':int(mismatch),'comparison_performed':performed,
            'gq_forced_zero':bool(no_call_non_y or mismatch),
            'reason':'control_mismatch' if mismatch else 'expected_profile_missing_or_empty' if not performed else 'labels_match',
            'comparison':'equal_count_and_case_insensitive_membership_no_consumption',
            'external_validation':False}


def zero_peak_metric(candidate_count,called,*,quality_sensor,expected):
    """Pre-cutoff candidate count and post-binning labels are separate inputs."""
    if type(candidate_count) is not int or not 0<=candidate_count<=4096:
        raise ValueError('candidate_count must be an integer in [0,4096]')
    called,expected=_labels(called,'called'),_labels(expected,'expected')
    _bool(quality_sensor,'quality_sensor')
    metric=1 if candidate_count==0 and not (quality_sensor and expected) else 0
    sensor_match=bool(quality_sensor and called and expected and sorted(called)==sorted(expected))
    if sensor_match:metric=1
    return {'value':metric,'quality_sensor_expected_match':sensor_match,
            'candidate_count':candidate_count,'case_sensitive_sensor_comparison':True}


def negative_control(zero_peak_value):
    value=_number(zero_peak_value,'QC_ZERO_PEAK',-3,1e6)
    return {'cc':int(value==0),'gq_forced_zero':value==0,
            'zero_peak_value':value,'scope':'does_not_recount_calls_or_apply_spike_weight'}


def dimensions_in_bp(peak,calibration):
    """Convert widths using rounded endpoint span / scan span, not local slope.

    Independent rounding is Python decimal rounding to5 places. Original tie
    rounding parity and out-of-anchor extrapolation are not claimed.
    """
    if not isinstance(peak,Mapping):raise ValueError('Peak object required')
    begin=_number(peak.get('begin_scan'),'begin_scan',0,1e8)
    end=_number(peak.get('end_scan'),'end_scan',0,1e8)
    if end<=begin:raise ValueError('Positive scan span required')
    left,right=calibration.size(begin),calibration.size(end)
    if left is None or right is None:
        return {'status':'unavailable','reason':'peak_bounds_outside_calibration','fwhm_bp':None,'area_bp':None}
    left,right=round(_number(left,'begin_bp',0,1e6),5),round(_number(right,'end_bp',0,1e6),5)
    scale=(right-left)/(end-begin)
    if scale<=0:raise ValueError('Positive calibrated endpoint span required')
    fwhm=_number(peak.get('fwhm_scan'),'fwhm_scan',0,1e8)
    area=_number(peak.get('area'),'area',0,1e20)
    return {'status':'evaluated','begin_bp':left,'end_bp':right,'bp_per_scan_span':scale,
            'fwhm_bp':scale*fwhm,'area_bp':scale*area,'original_rounding_parity':False}


def broad_standard_penalty(score,peaks,*,reference_channel,warning_width_bp,weight,is_hid,sq_overridden):
    score=_number(score,'score',0,1)
    threshold=_number(warning_width_bp,'warning_width_bp',0,1e6)
    weight=_number(weight,'weight',0,1)
    _bool(is_hid,'is_hid');_bool(sq_overridden,'sq_overridden')
    if not isinstance(reference_channel,str) or not reference_channel:
        raise ValueError('Explicit reference channel required')
    if not isinstance(peaks,(list,tuple)) or len(peaks)>4096:
        raise ValueError('At most4096 peaks required')
    result={'score':score,'broad_metric':None,'applied':False,'trigger_index':None,
            'reason':None,'calibration_rejection_requested':False}
    if not is_hid or sq_overridden:
        result['reason']='not_hid' if not is_hid else 'sq_overridden'
        return result
    result['broad_metric']=0
    for index,peak in enumerate(peaks):
        if not isinstance(peak,Mapping):raise ValueError('Peak object required')
        if peak.get('channel')!=reference_channel or peak.get('size_matched') is not True:continue
        width=_number(peak.get('fwhm_bp'),'matched standard FWHM bp',0,1e6)
        if width>threshold:
            result.update(score=(1-weight)*score,broad_metric=1,applied=True,trigger_index=index)
            break
    # Original stage is after sizing. Crossing a quality threshold here does
    # not itself mean the earlier calibration was absent or should be removed.
    return result
