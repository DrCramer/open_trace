"""Independent fresh true-peak model from side crossovers and accepted pairs.

All scan coordinates remain in the original companion domain. Polynomial fits
use the explicitly named independent SVD policy; no JVM numeric parity is
claimed. This is separate from the ideal-Gaussian correlation template.
"""
import math
from .quality_control import _number
from .rounding import round_scan


def two_point_gaussian(first, second, *, height_cap, denominator_limit):
    """A*exp(-x*x/C) about an already chosen center; strict C and A gates."""
    height_cap=_number(height_cap,'height cap',0,1e12)
    denominator_limit=_number(denominator_limit,'Gaussian denominator limit',1e-12,1e12)
    points=[]
    for point in (first,second):
        if not isinstance(point,(list,tuple)) or len(point)!=2:raise ValueError('Two coordinate pairs required')
        points.append((_number(point[0],'recentered scan',-1e8,1e8),_number(point[1],'point height',-1e12,1e12)))
    (x1,y1),(x2,y2)=points
    if y1<=0 or y2<=0:return None
    l1,l2=math.log(y1),math.log(y2)
    delta=l2-l1
    if delta==0:return None
    c=(x1*x1-x2*x2)/delta
    if not 0<c<denominator_limit:return None
    try:a=math.exp(l1+x1*x1/c)
    except OverflowError:return None
    if not 0<a<=height_cap:return None
    return {'height':a,'denominator_scan2':c}


def _collector(signal,lo,hi,start,step,center):
    from .small_svd import polynomial_fit,SmallSVDError
    result={'status':'no_crossover','intersection':None,'maximum':None,
        'consecutive':[],'gaussian':[],'last_scanned':None,'line_fit':None}
    positions=[start,start-step]
    third=start-2*step
    if (step>0 and third>=center) or (step<0 and third<=center):positions.append(third)
    if any(not lo<=p<=hi for p in positions):
        result.update(status='unavailable',reason='collector_line_points_outside_bounds');return result
    if len(positions)==2:
        slope=(signal[positions[1]]-signal[positions[0]])/(positions[1]-positions[0])
        intercept=signal[start]-slope*start
        result['line_fit']={'solver':'two_point_line','coefficients':[intercept,slope]}
    else:
        try:fit=polynomial_fit(positions,[signal[p] for p in positions],1)
        except SmallSVDError as error:
            result.update(status='unavailable',reason='collector_polynomial_unavailable',error=str(error));return result
        intercept,slope=fit['coefficients'];result['line_fit']=fit
    index=start+step
    while lo<=index<=hi:
        result['last_scanned']=index
        value=signal[index]
        if value>slope*index+intercept or value<=0:
            if value<=0:return result
            break
        index+=step
    else:return result
    first=(index,float(signal[index]));maximum=first
    consecutive=[first];gaussian=[first];last_height=first[1]
    result.update(status='collected',intersection=first)
    index+=step
    while lo<=index<=hi and (len(consecutive)<3 or len(gaussian)<4):
        result['last_scanned']=index
        point=(index,float(signal[index]))
        if point[1]>maximum[1]:maximum=point
        if len(consecutive)<3:consecutive.append(point)
        if len(gaussian)<4 and 0<point[1]<last_height:
            gaussian.append(point);last_height=point[1]
        index+=step
    result.update(maximum=maximum,consecutive=consecutive,gaussian=gaussian)
    return result


def _evaluate(model,scan):
    delta=scan-model['center']
    return model['height']*math.exp(-(delta*delta)/model['denominator_scan2'])


def _crossing(signal,lo,hi,center,step,level):
    index=center+step
    while lo<=index<=hi:
        if signal[index]<=level:return index,False
        index+=step
    return (lo if step<0 else hi),True


def _true_width(signal,lo,hi,model,height,fraction):
    center=round_scan(model['center'])
    result={'width_scan':-1.0,'reason':'true_width_gate','rounded_center':center}
    if not (lo<center<hi and model['height']<height):return result
    level=model['height']*fraction
    left,lc=_crossing(signal,lo,hi,center,-1,level)
    right,rc=_crossing(signal,lo,hi,center,1,level)
    l,r=float(left),float(right)
    difference=signal[left+1]-signal[left]
    if not lc and difference>0:l+=(level-signal[left])/difference
    difference=signal[right-1]-signal[right]
    if not rc and difference>0:r-=(level-signal[right])/difference
    lw,rw=center-l,r-center
    width=2*rw+1 if lc and not rc else 2*lw+1 if rc and not lc else lw+rw+1
    result.update(width_scan=width,reason=None,threshold=level,crossings=[left,right],
        interpolated_crossings=[l,r],truncated=[lc,rc])
    return result


def _model_from_sides(left,right,lo,hi,height,limit):
    from .small_svd import polynomial_fit,SmallSVDError
    candidate=right['maximum']
    source='right_measured' if candidate is not None else 'midpoint'
    if candidate is None:
        candidate=left['maximum']
        if candidate is not None:source='left_measured'
    elif left['maximum'] is not None and left['maximum'][1]>candidate[1]:
        candidate=left['maximum'];source='left_measured'
    quadratic=None;quadratic_reason='insufficient_two_sided_consecutive_points'
    if len(left['consecutive'])>1 and len(right['consecutive'])>1:
        points=left['consecutive']+right['consecutive']
        try:
            fit=polynomial_fit([p[0] for p in points],[p[1] for p in points],2)
            quadratic={'fit':fit,'vertex':None,'accepted':False}
            a0,a1,a2=fit['coefficients']
            # Finite-input policy explicitly excludes the original NaN vertex.
            if a2!=0:
                x=-a1/(2.0*a2)
                if math.isfinite(x) and left['intersection'][0]<=x<=right['intersection'][0]:
                    y=a2*x*x+a1*x+a0
                    if math.isfinite(y):
                        quadratic.update(vertex=[x,y],accepted=True)
                        if candidate is None or y>candidate[1]:candidate=(x,y);source='quadratic'
            quadratic_reason=None
        except SmallSVDError:quadratic_reason='quadratic_polynomial_unavailable'
    center=float((lo+hi)//2) if candidate is None else float(candidate[0])
    merged=[]
    for index in range(max(len(left['gaussian']),len(right['gaussian']))):
        for side in (left,right):
            if index<len(side['gaussian']):merged.append(side['gaussian'][index])
    accepted=[];attempts=0
    if len(merged)>2:
        for i in range(len(merged)):
            for j in range(i+1,len(merged)):
                attempts+=1
                a,b=merged[i],merged[j]
                fit=two_point_gaussian((a[0]-center,a[1]),(b[0]-center,b[1]),height_cap=height,denominator_limit=limit)
                if fit is not None:
                    accepted.append({'indices':[a[0],b[0]],**fit})
                    if len(accepted)==4:break
            if len(accepted)==4:break
    model=None
    if accepted:
        a,c=accepted[0]['height'],accepted[0]['denominator_scan2']
        for fit in accepted[1:]:a+=fit['height'];c+=fit['denominator_scan2']
        model={'height':a/len(accepted),'center':center,'denominator_scan2':c/len(accepted)}
    return model,{'selected_center':center,'center_source':source,'quadratic':quadratic,
        'quadratic_reason':quadratic_reason,'merged_indices':[p[0] for p in merged],
        'attempted_pairs':attempts,'accepted_pairs':accepted}


def _areas(signal,lo,hi,selection,whole,model,left_intersection,right_intersection):
    result={'area':-1.0,'excess_area_ratio':-1.0,'reason':'relative_intersections_missing',
            'relative_intersections':None}
    origin=selection['origin'];top=selection['relative_center']
    lc,rc=selection['truncated']
    left=None if left_intersection is None else left_intersection[0]-origin
    right=None if right_intersection is None else right_intersection[0]-origin
    if lc and not rc and right is not None:left=2*top-right
    elif rc and not lc and left is not None:right=2*top-left
    if left is None or right is None:return result
    if len(whole)<3:
        result['reason']='extracted_support_shorter_than_three';return result
    total=0.0
    for value in whole:total+=abs(value)
    area=0.0
    for index,value in enumerate(whole):
        area+=_evaluate(model,index+origin) if left<index<right else abs(value)
    left_tail=right_tail=0.0
    if not lc:
        for index in range(lo,int(origin)):left_tail+=abs(signal[index])
        if rc:left_tail*=2.0
        area+=left_tail
    if not rc:
        for index in range(len(whole)-1+int(origin),hi):right_tail+=abs(signal[index])
        if lc:right_tail*=2.0
        area+=right_tail
    excess=-1.0
    if total>0:
        excess=(selection['height']-_evaluate(model,selection['center']))*(right-left)/2.0/total
    result.update(area=area,excess_area_ratio=excess,reason=None,relative_intersections=[left,right],
        raw_extracted_area=total,left_tail_added=left_tail,right_tail_added=right_tail)
    return result


def _measure_true_peak(signal,lo,hi,selection,whole,definition):
    from .small_svd import SOLVER_ID
    result={'status':'unavailable','reason':'no_accepted_gaussian_pairs','height':-1.0,'center':-1.0,
        'width_scan':-1.0,'area':-1.0,'excess_area_ratio':-1.0,'model':None,
        'polynomial_solver':SOLVER_ID,'scope':'fresh_baseline_corrected_companion_offset0',
        'original_runtime_validated':False}
    sides=[]
    for index,step in enumerate((-1,1)):
        if selection['truncated'][index]:
            side={'status':'near_base_truncated','intersection':None,'maximum':None,
                'consecutive':[],'gaussian':[],'last_scanned':None,'line_fit':None}
        else:
            side=_collector(signal,lo,hi,selection['half_crossings'][index],step,selection['center'])
        sides.append(side)
    left,right=sides
    model,fit=_model_from_sides(left,right,lo,hi,selection['height'],definition['max_denominator_scan2'])
    result.update(fitting=fit,left_intersection=None if left['intersection'] is None else list(left['intersection']),
        right_intersection=None if right['intersection'] is None else list(right['intersection']))
    # Retain bounded point identities and fits, not copies of companion arrays.
    result['sides']=[{**{k:v for k,v in side.items() if k not in ('consecutive','gaussian')},
        'consecutive_indices':[p[0] for p in side['consecutive']],
        'gaussian_indices':[p[0] for p in side['gaussian']]} for side in sides]
    for side in result['sides']:
        for key in ('intersection','maximum'):
            if side[key] is not None:side[key]=list(side[key])
    if model is None:return result
    width=_true_width(signal,lo,hi,model,selection['height'],definition['near_base_fraction'])
    area=_areas(signal,lo,hi,selection,whole,model,left['intersection'],right['intersection'])
    result.update(status='evaluated',reason=None,height=model['height'],center=model['center'],
        width_scan=width['width_scan'],area=area['area'],excess_area_ratio=area['excess_area_ratio'],
        model=model,width_measurement=width,area_measurement=area)
    return result


def restore_peak(peak,classification,*,calibration,signal_length):
    """Return a copied fresh peak and exact write history for the public picker.

    Full-trace offset0 only. Map evaluation uses our existing model-dependent
    unrounded standard-map API. Added finite/bounds checks are explicit adapter
    limits; the original picker did not prevalidate both endpoint indices.
    """
    from .fastcopy import fastcopy
    if not isinstance(peak,dict) or not isinstance(classification,dict):raise ValueError('Peak and classification required')
    if 'true_peak_restoration' in peak:raise ValueError('Fresh peak required; recalculate from the source FSA')
    if type(signal_length) is not int or not 1<=signal_length<=1_000_000:raise ValueError('Bounded signal length required')
    result=fastcopy(peak)
    fields=('height','scan','size_bp','area','area_bp')
    history={'status':'not_applicable','reason':'public_restore_branch_not_selected',
        'scope':'fresh_non_reference_offset0','prior_state':'fresh_false',
        'before':{key:peak.get(key) for key in fields},'after':None,'writes':[],
        'original_runtime_validated':False,'reference_map_policy':'unrounded_model_dependent_standard_map'}
    def finish():
        history['after']={key:result.get(key) for key in fields}
        result['true_peak_restoration']=history
        return result
    if (classification.get('status')!='evaluated_subset' or classification.get('true_peak_restoration_requested') is not True
            or classification.get('spike_detected') is not False or classification.get('overlap_spike') is not True
            or classification.get('current_spike') is not False):
        return finish()
    model=(peak.get('spike_features') or {}).get('morphology',{}).get('true_peak',{})
    height=model.get('height',-1)
    if not isinstance(height,(int,float)) or isinstance(height,bool) or not math.isfinite(height) or height<=0:
        history.update(status='unavailable',reason='positive_true_peak_height_unavailable');return finish()
    result['height']=height;history['writes'].append('height')
    history.update(status='height_only',reason='size_call_map_unavailable')
    if calibration is None:return finish()
    values=(model.get('center'),peak.get('begin'),peak.get('end'),model.get('area',-1))
    if any(type(value) not in (int,float) or not math.isfinite(value) for value in values):
        history['reason']='nonfinite_restoration_geometry';return finish()
    center,begin,end,area=values
    c,b,e=int(center),int(begin),int(end)
    if abs(center)<=1e8:history['rounded_center_index']=round_scan(center)
    history.update(center_index=c,begin_index=b,end_index=e)
    if not (0<=c<signal_length and 0<=b<signal_length and 0<=e<signal_length):
        history['reason']='restoration_map_indices_outside_full_trace';return finish()
    if e<=b:
        history['reason']='nonpositive_integer_endpoint_span';return finish()
    try:left,right,position=(calibration.standard_map_size(index) for index in (b,e,c))
    except ValueError:
        history['reason']='size_call_map_evaluation_unavailable';return finish()
    if any(not math.isfinite(value) for value in (left,right,position)):
        history['reason']='nonfinite_size_call_map';return finish()
    slope=(right-left)/(e-b)
    area_bp=slope*area
    if not math.isfinite(area_bp):
        history['reason']='nonfinite_restored_area_bp';return finish()
    result.update(scan=c,size_bp=position,area=area,area_bp=area_bp)
    history['writes'].extend(('scan','size_bp','area','area_bp'))
    history.update(status='restored',reason=None,map_endpoints=[left,right],bp_per_scan_span=slope,
        area_available=area>=0,size_available=position>0)
    return finish()
