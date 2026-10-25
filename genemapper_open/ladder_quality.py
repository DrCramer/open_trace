"""Independent fresh ladder-marker quality gates from retained static contracts.

Explicit effective settings only; no commercial defaults or runtime. Null
tolerance denotes the original disabled comparison, not an infinite JSON value.
"""
from collections.abc import Mapping
from .fastcopy import fastcopy
from .quality_control import _number, _bool


def _object(value, keys, name):
    if not isinstance(value, Mapping) or set(value) != set(keys):
        raise ValueError(name+': required fields '+', '.join(keys))


def _name(value):
    if not isinstance(value,str) or not 1<=len(value)<=128 or not value.isascii():
        raise ValueError('Bounded ASCII allele/bin name required')
    return value


def _state(value):
    value=_number(value,'metric state',-2,1)
    if value not in (-2,-1,0,1):raise ValueError('Explicit metric state -2/-1/0/1 required')
    return value


def validate_ladder_recipe(recipe):
    _object(recipe,('min_height_ratio','spike_metric','spike_weight','offscale_metric','offscale_weight','spacing'),'ladder recipe')
    _number(recipe['min_height_ratio'],'min_height_ratio',0,1)
    for key in ('spike','offscale'):
        _state(recipe[key+'_metric']);_number(recipe[key+'_weight'],key+'_weight',0,1)
    spacing=recipe['spacing']
    _object(spacing,('enabled','max_distance_bp','curve','exceptions','excluded_alleles'),'spacing')
    _bool(spacing['enabled'],'spacing.enabled')
    _number(spacing['max_distance_bp'],'max_distance_bp',0,1e6)
    curve=spacing['curve']
    if not isinstance(curve,(list,tuple)) or len(curve)>128:raise ValueError('At most128 explicit curve nodes required')
    last=-1
    for node in curve:
        _object(node,('distance_bp','fraction','interpolate'),'curve node')
        x=_number(node['distance_bp'],'distance_bp',0,1e6)
        _number(node['fraction'],'fraction',-1e6,1e6);_bool(node['interpolate'],'interpolate')
        if x<=last:raise ValueError('Explicit curve distances must strictly increase')
        last=x
    pairs=spacing['exceptions']
    if not isinstance(pairs,(list,tuple)) or len(pairs)>4096:raise ValueError('At most4096 explicit pair exceptions required')
    seen=set()
    for pair in pairs:
        _object(pair,('first','second','tolerance_bp'),'pair exception')
        first,second=(_name(pair[key]).strip(''.join(map(chr,range(33)))) for key in ('first','second'))
        if not first or not second or (first,second) in seen:raise ValueError('Nonempty unique directed exception pairs required')
        seen.add((first,second));_number(pair['tolerance_bp'],'tolerance_bp',-1e6,1e6)
    excluded=spacing['excluded_alleles']
    if not isinstance(excluded,(list,tuple)) or len(excluded)>128:raise ValueError('At most128 explicitly excluded alleles required')
    for name in excluded:_name(name)
    return fastcopy(dict(recipe))


def _tolerance(distance,first,second,spacing):
    if distance>spacing['max_distance_bp']:return None,'distance_above_maximum'
    trim=''.join(map(chr,range(33)))
    first,second=first.strip(trim),second.strip(trim)
    pairs={(p['first'].strip(trim),p['second'].strip(trim)):p['tolerance_bp'] for p in spacing['exceptions']}
    explicit=pairs.get((first,second),pairs.get((second,first)))
    if explicit is not None and explicit>=0:return float(explicit),'pair_exception'
    index=int(distance)
    curve=spacing['curve']
    # Enabled checks always have a curve; the public helper also handles empty.
    if not curve:return None,'empty_curve'
    if index<=curve[0]['distance_bp']:fraction=curve[0]['fraction']
    elif index>=curve[-1]['distance_bp']:fraction=curve[-1]['fraction']
    else:
        for left,right in zip(curve,curve[1:]):
            if left['distance_bp']<=index<right['distance_bp']:
                fraction=left['fraction']
                if left['interpolate']:
                    fraction=right['fraction']+(index-right['distance_bp'])*(left['fraction']-right['fraction'])/(left['distance_bp']-right['distance_bp'])
                break
    value=float(fraction)*(index if index else 1)
    return (None,'negative_lookup_disabled') if value<0 else (value,'integer_distance_curve')


def ladder_spacing_tolerance(distance,first,second,recipe):
    """Inspect a validated effective marker spacing rule without invoking a gate."""
    recipe=validate_ladder_recipe(recipe)
    value,source=_tolerance(_number(distance,'bin distance',0,1e6),_name(first),_name(second),recipe['spacing'])
    return {'tolerance_bp':value,'source':source}


def score_ladder_marker(calls,expected_bins,recipe):
    """Fresh finite-positive-peak subset, before original overrides/rounding.

    Calls carry separate allele size and measured peak bp. Expected membership
    uses exact bin names, without consuming duplicates or requiring equal count.
    """
    recipe=validate_ladder_recipe(recipe)
    if not isinstance(expected_bins,(list,tuple)) or len(expected_bins)>128:
        raise ValueError('At most128 explicit expected bin names required')
    for label in expected_bins:_name(label)
    if not isinstance(calls,(list,tuple)) or len(calls)>4096:raise ValueError('At most4096 explicit calls required')
    prepared=[]
    for call in calls:
        _object(call,('name','size_bp','peak_bp','height','bin','off_ladder'),'ladder call')
        _name(call['name']);_number(call['size_bp'],'allele size',0,1e6)
        _number(call['peak_bp'],'peak bp',0,1e6);_number(call['height'],'height',1e-100,1e12)
        _bool(call['off_ladder'],'off_ladder')
        if call['bin'] is not None:
            _object(call['bin'],('name','center_bp'),'bin')
            _name(call['bin']['name']);_number(call['bin']['center_bp'],'bin center',0,1e6)
        prepared.append(fastcopy(call))
    prepared.sort(key=lambda c:c['size_bp'])
    result={'status':'evaluated_subset','gq':1.0,'ladder_metric':0,'allele_number_metric':-2,
        'recipe':recipe,'events':[],'spacing_checks':[],'sorted_call_names':[c['name'] for c in prepared],
        'first_two_height_ratio':None,'min_max_height_ratio':None,'missing_expected_bins':[],
        'scope':'fresh_finite_positive_peaks_without_overrides_or_original_rounding',
        'external_validation':False,'original_runtime_validated':False}
    def fail(reason,**details):
        result['ladder_metric']=1;result['gq']=0.0;result['events'].append({'criterion':reason,**details})
    if not prepared:fail('no_called_alleles')
    for key in ('spike','offscale'):
        if recipe[key+'_weight']>0 and recipe[key+'_metric']==1:fail(key+'_gate')
    names={c['bin']['name'] for c in prepared if c['bin'] is not None}
    missing=[name for name in expected_bins if name not in names]
    result['missing_expected_bins']=missing
    if missing:fail('expected_bin_missing',bins=missing)
    if len(prepared)>1:
        ratio=prepared[0]['height']/prepared[1]['height'];result['first_two_height_ratio']=ratio
        if ratio<=.5:fail('first_two_height_ratio',observed=ratio,threshold=.5)
        heights=[c['height'] for c in prepared]
        ratio=min(heights)/max(heights);result['min_max_height_ratio']=ratio
        if ratio<=recipe['min_height_ratio']:fail('min_max_height_ratio',observed=ratio,threshold=recipe['min_height_ratio'])
    spacing=recipe['spacing']
    result['spacing_enabled']=spacing['enabled'] and bool(spacing['curve'])
    excluded={s.lower() for s in spacing['excluded_alleles']}
    if result['spacing_enabled']:
        for index,first in enumerate(prepared[:-1]):
            if first['off_ladder'] or first['name'].lower() in excluded:continue
            second=None
            # Original loop leaves the last OL as its candidate if all remaining
            # calls are OL. Preserve that missing-bin failure instead of dropping it.
            for candidate in prepared[index+1:]:
                second=candidate
                if not candidate['off_ladder']:break
            if second is None:continue
            observation={'first':first['name'],'second':second['name']}
            result['spacing_checks'].append(observation)
            if first['bin'] is None or second['bin'] is None:
                observation.update(status='failed',reason='pair_bin_missing')
                fail('pair_bin_missing',first=first['name'],second=second['name']);break
            observed=abs(second['peak_bp']-first['peak_bp'])
            distance=abs(second['bin']['center_bp']-first['bin']['center_bp'])
            tolerance,source=_tolerance(distance,first['name'],second['name'],spacing)
            error=abs(observed-distance)
            observation.update(observed_bp=observed,bin_distance_bp=distance,error_bp=error,
                tolerance_bp=tolerance,tolerance_source=source,status='disabled' if tolerance is None else 'passed')
            if tolerance is not None and error>tolerance:
                observation['status']='failed';fail('bin_spacing',**observation);break
    return result
