"""Independent fresh spike classification with explicit measurement policies.

Schemas1/2 retain their prepared-signal contract. Schema3 measures companion
jag/ideal fit with overlap exclusion. Schema4 adds explicit fresh restoration.
"""
from collections.abc import Mapping
from .fastcopy import fastcopy
import json
import math
from .quality_control import _number,_bool
from .rounding import round_scan


def load_spike_definition(source):
    try:
        if isinstance(source,(str,bytes)):
            encoded=source.encode('utf-8') if isinstance(source,str) else source
            if len(encoded)>16384:raise ValueError('Spike definition exceeds16KiB')
            def pairs(items):
                result={}
                for key,value in items:
                    if key in result:raise ValueError('Duplicate spike JSON field: '+key)
                    result[key]=value
                return result
            def invalid(value):raise ValueError('Nonfinite JSON constant: '+value)
            source=json.loads(encoded,object_pairs_hook=pairs,parse_constant=invalid)
        elif len(json.dumps(source,allow_nan=False).encode())>16384:raise ValueError('Spike definition exceeds16KiB')
    except (TypeError,RecursionError,OverflowError,UnicodeError) as error:
        raise ValueError('Bounded spike definition JSON required') from error
    keys={'schema_version','measurement_source','scope','morphology_enabled','units','min_height',
          'exhalf_width_threshold','near_base_width_threshold','area_ratio_threshold',
          'treat_overlap_as_spike','near_base_fraction','max_base_height'}
    if not isinstance(source,Mapping):raise ValueError('Explicit spike definition required')
    version=source.get('schema_version')
    if type(version) is not int or version not in (1,2,3,4):raise ValueError('Spike schema_version must be1,2,3 or4')
    if version>=2:keys.add('adjustments')
    if version>=3:keys.add('morphology')
    if version==4:keys.add('true_peak')
    if set(source)!=keys:raise ValueError('All explicit spike fields required')
    if version>=2:
        from .spike_adjustment import validate_adjustments
        validate_adjustments(source['adjustments'])
    measurement='baseline_corrected_companion' if version>=3 else 'prepared_signal'
    if source['measurement_source']!=measurement or source['scope']!='non_reference_selected_channels':
        raise ValueError('Explicit '+measurement+'/non_reference_selected_channels policy required')
    if source['morphology_enabled'] is not (version>=3):raise ValueError('Morphology policy must match spike schema')
    if version>=3:
        if version==3 and source['treat_overlap_as_spike'] is not True:
            raise ValueError('Morphology subset requires treat_overlap_as_spike=true; true-peak restoration is not implemented')
        parameters=source['morphology']
        if not isinstance(parameters,Mapping) or set(parameters)!={'jag_threshold','fit_score_threshold'}:
            raise ValueError('Explicit jag_threshold and fit_score_threshold required')
        _number(parameters['jag_threshold'],'jag_threshold',0,1e12)
        _number(parameters['fit_score_threshold'],'fit_score_threshold',0,1)
    if version==4:
        from .small_svd import SOLVER_ID
        parameters=source['true_peak']
        if not isinstance(parameters,Mapping) or set(parameters)!={'polynomial_solver','max_denominator_scan2'}:
            raise ValueError('Explicit true_peak solver and Gaussian denominator bound required')
        if parameters['polynomial_solver']!=SOLVER_ID:raise ValueError('Unsupported explicit true-peak polynomial solver')
        _number(parameters['max_denominator_scan2'],'max_denominator_scan2',1e-12,1e12)
    if source['units'] not in ('scan','bp'):raise ValueError('Explicit scan or bp units required')
    _bool(source['treat_overlap_as_spike'],'treat_overlap_as_spike')
    for key in ('min_height','exhalf_width_threshold','near_base_width_threshold','area_ratio_threshold'):
        _number(source[key],key,0,1e12)
    _number(source['near_base_fraction'],'near_base_fraction',1e-12,1)
    _number(source['max_base_height'],'max_base_height',-1,1e12)
    return fastcopy(dict(source))


def _width_out(signal,lo,hi,center,ratio,cap):
    """Normal searchMax route: widened widthOut and mirrored integer areaOut."""
    maximum=max(signal[lo:hi+1]);apex=next(i for i in range(lo,hi+1) if signal[i]==maximum)
    if apex!=center:
        original_distance=abs(apex-center)
        for i in range(lo,hi+1):
            if signal[i]==maximum and abs(i-center)<original_distance:apex=i
    if apex in (lo,hi):apex=center;maximum=signal[center]
    if maximum<=0:return {'width_scan':-1.0,'area':-1.0,'height':-1.0,'center':apex}
    level=ratio*maximum
    if cap>=0:level=min(level,cap)
    left,right=apex-1,apex+1
    while left>=lo and signal[left]>level:left-=1
    while right<=hi and signal[right]>level:right+=1
    truncated_left,truncated_right=left<lo,right>hi
    left_index=max(left,lo);right_index=min(right,hi)
    if truncated_left:left_cross=float(lo)
    else:
        difference=signal[left+1]-signal[left]
        left_cross=left+(level-signal[left])/difference if difference>0 else float(left)
    if truncated_right:right_cross=float(hi)
    else:
        difference=signal[right-1]-signal[right]
        right_cross=right-(level-signal[right])/difference if difference>0 else float(right)
    left_width,right_width=apex-left_cross,right_cross-apex
    left_area=sum(signal[left_index:apex]);right_area=sum(signal[apex+1:right_index+1])
    if truncated_left and not truncated_right:
        left_width=max(left_width,right_width);left_area=right_area
    elif truncated_right and not truncated_left:
        right_width=max(left_width,right_width);right_area=left_area
    return {'width_scan':left_width+right_width,'area':left_area+right_area+signal[apex],
            'height':float(maximum),'center':apex,'left_truncated':truncated_left,'right_truncated':truncated_right}


def measure_spike_features(signal,peak,definition):
    """Use fractional SG bounds before wrapper projection; never ordinary FWHM."""
    definition=load_spike_definition(definition)
    _signal(signal)
    return _measure_features(signal,peak,definition)


def _signal(signal):
    if not isinstance(signal,(list,tuple)) or not 1<=len(signal)<=1_000_000:
        raise ValueError('Bounded prepared integer signal required')
    if any(type(v) is not int or not 0<=v<2**31 for v in signal):raise ValueError('Prepared signal must be nonnegative signed32 integers')


def measure_spike_peaks(signal,peaks,definition):
    """Batch validation of the signal once; return only scalar measurements."""
    definition=load_spike_definition(definition);_signal(signal)
    if not isinstance(peaks,(list,tuple)) or len(peaks)>4096:raise ValueError('At most4096 SG peaks required')
    return [_measure_features(signal,peak,definition) for peak in peaks]


def _measure_features(signal,peak,definition):
    if not isinstance(peak,Mapping):raise ValueError('Measured SG peak required')
    result={'status':'unavailable','reason':None,'measurement_source':definition['measurement_source'],
        'exhalf_width_scan':None,'near_base_width_scan':None,'near_base_area':None,'raw_height':None,
        'area_ratio':None,'jag_count':-1,'jag_count_source':'fresh_uncomputed_sentinel_morphology_disabled',
        'external_validation':False,'original_runtime_validated':False}
    begin=_number(peak.get('begin'),'begin',0,len(signal)-1)
    end=_number(peak.get('end'),'end',0,len(signal)-1)
    crossing=_number(peak.get('sg_scan'),'SG crossing',0,len(signal)-1)
    lo,hi,center=math.ceil(begin),math.floor(end),round_scan(crossing)
    if not lo<=center<=hi:
        result['reason']='sg_measurement_bounds_unavailable';return result
    half=_width_out(signal,lo,hi,center,.5,-1)
    base=_width_out(signal,lo,hi,center,definition['near_base_fraction'],definition['max_base_height'])
    ratio=half['width_scan']*half['height']/base['area'] if base['area']>0 else -1.0
    result.update(status='evaluated_subset',exhalf_width_scan=half['width_scan'],near_base_width_scan=base['width_scan'],
        near_base_area=base['area'],raw_height=half['height'],area_ratio=ratio,measurement_center=half['center'],
        half_truncation=[half.get('left_truncated',False),half.get('right_truncated',False)],
        base_truncation=[base.get('left_truncated',False),base.get('right_truncated',False)])
    if definition['schema_version']>=3:
        from .peak_morphology import _measure
        morphology=_measure(signal,lo,hi,center,half['width_scan'],definition)
        result.update(morphology=morphology,jag_count=morphology['jag_count'],
            jag_count_source='measured_companion' if morphology['jag_count']>=0 else 'morphology_uncomputed_sentinel',
            fit_score=morphology['fit_score'],area_ratio=morphology['area_ratio'])
    return result


def classify_spike(features,*,height,offscaled,definition):
    """Fresh public-picker subset; schema3 requires overlap exclusion policy."""
    definition=load_spike_definition(definition)
    height=_number(height,'height',0,1e12);_bool(offscaled,'offscaled')
    result={'status':'unavailable','reason':None,'spike_detected':None,'overlap_spike':None,
        'current_spike':None,'threshold_used':None,'units':definition['units'],
        'scope':'fresh_basic_initial_classification_before_downstream_adjustments',
        'external_validation':False,'original_runtime_validated':False}
    suffix=definition['units']
    if not isinstance(features,Mapping) or features.get('status')!='evaluated_subset':
        result['reason']='basic_spike_features_missing';return result
    names=('exhalf_width_'+suffix,'near_base_width_'+suffix,'area_ratio','jag_count')
    if any(features.get(key) is None for key in names):
        result['reason']='spike_units_or_features_unavailable';return result
    f,b,r,j=(_number(features[key],key,-1,1e12) for key in names)
    detected=overlap=False;threshold=definition['near_base_width_threshold'];reason='widths_not_narrow'
    if b>=0 and height>definition['min_height']:
        if b<definition['near_base_width_threshold']:
            detected=True;reason='near_base_width_below_threshold'
        elif f>=0 and f<definition['exhalf_width_threshold']:
            if r>definition['area_ratio_threshold'] and not offscaled:
                detected=True;threshold=definition['exhalf_width_threshold'];reason='narrow_exhalf_with_area_ratio'
            else:overlap=True;reason='narrow_exhalf_overlap'
        elif j>0:overlap=True;reason='positive_jag_count'
    else:reason='height_or_base_width_gate'
    current=detected or (definition['treat_overlap_as_spike'] and overlap)
    if overlap and not detected and definition['treat_overlap_as_spike']:threshold=definition['exhalf_width_threshold']
    result.update(status='evaluated_subset',reason=reason,spike_detected=detected,overlap_spike=overlap,
                  current_spike=current,threshold_used=threshold)
    if definition['schema_version']>=3:
        # Jag/fit branch is reached only after both narrow-width branches fail.
        broad=(b>=0 and height>definition['min_height'] and b>=definition['near_base_width_threshold']
               and not (0<=f<definition['exhalf_width_threshold']))
        fit=features.get('fit_score')
        if fit is None:
            result.update(status='unavailable',reason='morphology_fit_score_missing',spike_detected=None,
                          overlap_spike=None,current_spike=None,threshold_used=None)
            return result
        fit=_number(fit,'fit_score',-1,1e12)
        result.update(peak_deformity='poor_fit' if broad and fit<definition['morphology']['fit_score_threshold'] else 'none',
            scope='fresh_companion_jag_fit_initial_classification_overlap_excluded',
            true_peak_restoration_applied=False)
        if definition['schema_version']==4:
            result.update(scope='fresh_companion_true_peak_initial_classification',
                true_peak_restoration_requested=not detected and not definition['treat_overlap_as_spike'] and overlap)
    return result
