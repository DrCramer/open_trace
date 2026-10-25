"""Fresh HID SQ after calibrated matched-standard morphology.

The base score is the existing independent sizing-quality result. The broad
penalty and model-dependent exterior map rules come from retained ay/ah/o/af
static contracts. No manufacturer thresholds, overrides or genotype truth.
"""
from collections.abc import Mapping
from .fastcopy import fastcopy
import hashlib
import json
from .quality_control import _number,quality_category,broad_standard_penalty


def load_sizing_quality_definition(source):
    try:
        if isinstance(source,(str,bytes)):
            encoded=source.encode('utf-8') if isinstance(source,str) else source
            if len(encoded)>16384:raise ValueError('Sizing quality definition exceeds16KiB')
            def pairs(items):
                result={}
                for key,value in items:
                    if key in result:raise ValueError('Duplicate sizing quality JSON field: '+key)
                    result[key]=value
                return result
            def invalid(value):raise ValueError('Nonfinite JSON constant: '+value)
            source=json.loads(encoded,object_pairs_hook=pairs,parse_constant=invalid)
        elif len(json.dumps(source,allow_nan=False).encode())>16384:
            raise ValueError('Sizing quality definition exceeds16KiB')
    except (TypeError,RecursionError,OverflowError,UnicodeError) as error:
        raise ValueError('Bounded sizing quality definition required') from error
    keys={'schema_version','context','width_conversion','warning_width_bp','broad_weight','thresholds'}
    if not isinstance(source,Mapping) or set(source)!=keys:raise ValueError('All explicit post-sizing quality fields required')
    if type(source['schema_version']) is not int or source['schema_version']!=1:raise ValueError('Sizing quality schema_version must be1')
    if source['context']!='fresh_hid_no_override':raise ValueError('Only explicit fresh_hid_no_override context supported')
    if source['width_conversion']!='unrounded_standard_map_span':raise ValueError('Explicit unrounded_standard_map_span policy required')
    _number(source['warning_width_bp'],'warning_width_bp',0,1e6)
    _number(source['broad_weight'],'broad_weight',0,1)
    thresholds=source['thresholds']
    if not isinstance(thresholds,Mapping) or set(thresholds)!= {'good','bad'}:raise ValueError('Explicit good/bad SQ thresholds required')
    quality_category(.5,**thresholds)
    return fastcopy(dict(source))


def standard_dimensions(peak,calibration):
    """Use integer wrapper bounds, unrounded standard map and endpoint span.

    Negative/sentinel endpoints or nonpositive scale remain unavailable. Their
    downstream original degeneracy behavior is not silently replaced by zero.
    """
    begin=_number(peak.get('begin'),'begin',0,1e8);end=_number(peak.get('end'),'end',0,1e8)
    width=_number(peak.get('fwhm_scan'),'fwhm_scan',0,1e8)
    lo,hi=int(begin),int(end)
    result={'status':'unavailable','reason':None,'begin_scan':lo,'end_scan':hi,'fwhm_scan':width,
        'begin_bp':None,'end_bp':None,'bp_per_scan_span':None,'fwhm_bp':None,
        'position_rounding_applied':False,'outside_anchor_bounds':None}
    if hi<=lo:result['reason']='nonpositive_integer_peak_span';return result
    anchors=calibration.anchors
    result['outside_anchor_bounds']=lo<anchors[0][0] or hi>anchors[-1][0]
    left,right=calibration.standard_map_size(lo),calibration.standard_map_size(hi)
    result.update(begin_bp=left,end_bp=right)
    if left<0 or right<0:result['reason']='negative_or_sentinel_standard_map_endpoint';return result
    scale=(right-left)/(hi-lo)
    if scale<=0:result['reason']='nonpositive_standard_map_span';return result
    value=scale*width
    _number(value,'calibrated standard FWHM',0,1e6)
    result.update(status='evaluated',bp_per_scan_span=scale,fwhm_bp=value)
    return result


def evaluate_post_sizing_quality(base,selected_peaks,calibration,definition,*,reference_channel):
    definition=load_sizing_quality_definition(definition)
    encoded=json.dumps(definition,sort_keys=True,separators=(',',':'),ensure_ascii=False,allow_nan=False).encode()
    result={'schema_version':1,'status':'unavailable','reason':None,'definition_sha256':hashlib.sha256(encoded).hexdigest(),
        'base_score':None,'score':None,'broad_metric':None,'category':None,'penalty':None,'measurements':[],
        'context':definition['context'],'width_conversion':definition['width_conversion'],
        'scope':'independent_base_score_plus_matched_standard_broad_penalty',
        'full_original_sq':False,'calibration_rejection_requested':False,
        'external_validation':False,'original_runtime_validated':False}
    if not isinstance(base,Mapping) or base.get('status')!='evaluated':
        result['reason']='base_sizing_quality_unavailable';return result
    result['base_score']=_number(base.get('score'),'base sizing score',0,1)
    if calibration is None:
        result['reason']='accepted_calibration_unavailable';return result
    model=calibration.diagnostics['model']
    result.update(calibration_model=model,exterior_rule='fitted_polynomial' if model in ('lsq2','lsq3') else
                  'independent_piecewise_linear_secant' if model=='linear' else 'endpoint_pair_secant_negative_sentinel')
    if not isinstance(selected_peaks,(list,tuple)) or len(selected_peaks)>4096:
        raise ValueError('At most4096 selected standard peaks required')
    matched=[]
    for index,peak in enumerate(selected_peaks):
        if not isinstance(peak,Mapping):raise ValueError('Selected standard peak object required')
        if peak.get('matched_bp') is None:continue
        measured=standard_dimensions(peak,calibration)
        measured.update(selected_peak_index=index,scan=peak['scan'],matched_bp=peak['matched_bp'])
        result['measurements'].append(measured)
        if measured['status']=='evaluated':matched.append({'channel':reference_channel,'size_matched':True,
            'fwhm_bp':measured['fwhm_bp']})
    if not result['measurements']:
        result['reason']='matched_standard_measurements_missing';return result
    if len(matched)!=len(result['measurements']):
        result['reason']='matched_standard_dimensions_unavailable';return result
    penalty=broad_standard_penalty(result['base_score'],matched,reference_channel=reference_channel,
        warning_width_bp=definition['warning_width_bp'],weight=definition['broad_weight'],is_hid=True,sq_overridden=False)
    result.update(status='evaluated',score=penalty['score'],broad_metric=penalty['broad_metric'],penalty=penalty,
        category=quality_category(penalty['score'],**definition['thresholds']))
    return result
