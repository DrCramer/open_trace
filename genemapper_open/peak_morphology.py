"""Independent jag and ideal-Gaussian correlation measurements.

This finite-input subset uses an explicit baseline-corrected companion before
smoothing. Schema4 separately delegates true-peak fitting; the ideal template
is unchanged. Sampling order, inclusive crossings and full-N jag are intentional.
"""
import math
from .quality_control import _number


def jag_metrics(values, threshold):
    """Normalized second differences; unavailable is distinct from zero jag."""
    threshold = _number(threshold, 'jag threshold', 0, 1e12)
    _values(values)
    return _jag(values, threshold)


def _values(values):
    if not isinstance(values, (list, tuple)) or len(values) > 2_000_001:
        raise ValueError('Bounded numeric morphology samples required')
    for value in values:
        _number(value, 'morphology sample', -2**31, 2**31-1)


def _jag(values, threshold):
    result = {'score': -1.0, 'count': -1, 'maximum': -1.0}
    if len(values) < 3 or max(values) <= 0:
        return result
    height = float(max(values))
    total, count, maximum = 0.0, 0, -1.0
    for index in range(1, len(values)-1):
        change = abs(2.0*values[index]-values[index+1]-values[index-1])/2.0/height
        total += change
        count += int(change > threshold)
        maximum = max(maximum, change)
    return {'score': total/len(values), 'count': count, 'maximum': maximum}


def gaussian_samples(values, width, height, center):
    """Ideal Gaussian vs linearly interpolated observations, in original order.

    The literal target30 and adjustment0.11 describe the algorithm, not a kit.
    The result may have fewer than30 pairs. Invalid/nonpositive width is rejected
    explicitly; nonfinite original floating outcomes are outside this subset.
    """
    _values(values)
    width = _number(width, 'exhalf width', 1e-12, 1e8)
    height = _number(height, 'extracted height', 0, 2**31-1)
    center = _number(center, 'relative center', 0, max(0, len(values)-1))
    if len(values) < 2:
        return [], []
    sigma = width/2.35482
    denominator = sigma*2.0*sigma
    extra, step = max(0, 30-len(values)), 0.0
    if extra:
        step = (len(values)-1)/extra
        if step >= 1 and step-int(step) < .11:
            step = int(step)+.11
        if step == 0:
            step = .11
        extra = max(0, int((len(values)-1)/step)-1)
    expected, observed = [], []

    def append(position, value):
        expected.append(height*math.exp(-((position-center)*(position-center))/denominator))
        observed.append(float(value))

    for index, value in enumerate(values):
        append(float(index), value)
    position = step
    for _ in range(extra):
        lower, upper = math.floor(position), math.ceil(position)
        a, b = (values[lower], values[upper]) if upper < len(values) else (0.0, 0.0)
        append(position, a+(b-a)*(position-lower))
        position += step
    return expected, observed


def gaussian_fit_score(values, width, height, center):
    """Squared Pearson correlation, with exact zero-variance result1; no clamp."""
    expected, observed = gaussian_samples(values, width, height, center)
    if not expected:
        return -1.0
    se = so = seo = see = soo = 0.0
    for e, o in zip(expected, observed):
        se += e
        so += o
        seo += e*o
        see += e*e
        soo += o*o
    count = len(expected)
    covariance = seo-se*so/count
    ve, vo = see-se*se/count, soo-so*so/count
    return covariance*covariance/(ve*vo) if ve != 0 and vo != 0 else 1.0


def _extract(signal, lo, hi, center, fraction, cap):
    """Normal nonlegacy source route; scratch arrays never enter public reports."""
    height, selected = float(signal[lo]), center
    for index in range(lo+1, hi+1):
        if signal[index] > height:
            height, selected = float(signal[index]), index
    if selected == center:
        height = float(signal[center])
    else:
        distance = abs(selected-center)
        for index in range(lo, hi+1):
            if signal[index] == height and abs(index-center) < distance:
                selected = index
    if selected in (lo, hi):
        selected, height = center, float(signal[center])
    near = height*fraction
    if cap >= 0:
        near = min(near, cap)

    def crossing(step, level):
        index = selected+step
        while lo <= index <= hi:
            if signal[index] <= level:
                return index, False
            index += step
        return (lo if step < 0 else hi), True

    left, truncated_left = crossing(-1, near)
    right, truncated_right = crossing(1, near)
    half_left, _ = crossing(-1, height/2.0)
    half_right, _ = crossing(1, height/2.0)
    bottom_left, bottom_right = left, right
    if height/2.0 <= near or left > half_left or right < half_right:
        bottom_left, bottom_right = lo, hi
    if truncated_left and not truncated_right:
        whole = list(reversed(signal[selected:right+1]))+list(signal[selected+1:right+1])
        top = list(reversed(signal[selected:half_right+1]))+list(signal[selected+1:half_right+1])
        br = list(signal[half_right:bottom_right+1])
        bl = list(reversed(br))
        relative, origin = right-selected, 2*selected-right
    elif truncated_right and not truncated_left:
        whole = list(signal[left:selected+1])+list(reversed(signal[left:selected]))
        top = list(signal[half_left:selected+1])+list(reversed(signal[half_left:selected]))
        bl = list(signal[bottom_left:half_left+1])
        br = list(reversed(bl))
        relative, origin = selected-left, left
    else:
        whole = list(signal[left:right+1])
        top = list(signal[half_left:half_right+1])
        bl, br = list(signal[bottom_left:half_left+1]), list(signal[half_right:bottom_right+1])
        relative, origin = selected-left, left
    return whole, top, bl, br, {'height': height, 'center': selected, 'relative_center': relative,
        'origin': origin, 'near_crossings': [left, right], 'half_crossings': [half_left, half_right],
        'truncated': [truncated_left, truncated_right]}


def _measure(signal, lo, hi, center, width, definition):
    """Called only after spike module has validated signal/bounds/definition."""
    result = {'status': 'unavailable', 'reason': 'insufficient_integer_support',
        'jag_score': -1.0, 'jag_score_top': -1.0, 'jag_score_bottom': -1.0,
        'jag_count': -1, 'jag_max': -1.0, 'fit_score': -1.0, 'area_ratio': -1.0,
        'true_peak_restoration': 'not_implemented', 'original_runtime_validated': False}
    if hi-lo < 3:
        return result
    whole, top, left, right, selection = _extract(signal, lo, hi, center,
        definition['near_base_fraction'], definition['max_base_height'])
    if definition['schema_version']==4:
        from .true_peak import _measure_true_peak
        result['true_peak']=_measure_true_peak(signal,lo,hi,selection,whole,
            {**definition['true_peak'],'near_base_fraction':definition['near_base_fraction']})
        result['true_peak_restoration']='measured_separately_after_calibration'
    if len(whole) < 3:
        result['reason'] = 'extracted_support_shorter_than_three'
        return result
    parameters = definition['morphology']
    jag = _jag(whole, parameters['jag_threshold'])
    top_score = _jag(top, parameters['jag_threshold'])['score']
    if len(left) < 3 and len(right) < 3:
        bottom_score = top_score
    elif len(left) >= 3 and len(right) >= 3:
        bottom_score = (_jag(left, parameters['jag_threshold'])['score']+
                        _jag(right, parameters['jag_threshold'])['score'])/2.0
    else:
        bottom_score = _jag(left if len(left) >= 3 else right, parameters['jag_threshold'])['score']
    total = 0.0
    for value in whole:
        total += abs(value)
    # Morphology overwrites RArea even if ideal fitting is unavailable.
    ratio = width*selection['height']/total if total > 0 else -1.0
    fit = -1.0
    reason = None
    if width >= 1e-12:
        fit = gaussian_fit_score(whole, width, selection['height'], selection['relative_center'])
    else:
        reason = 'nonpositive_or_too_small_exhalf_width'
    result.update(status='evaluated_subset', reason=reason, jag_score=jag['score'],
        jag_score_top=top_score, jag_score_bottom=bottom_score, jag_count=jag['count'],
        jag_max=jag['maximum'], fit_score=fit, area_ratio=ratio, selection=selection,
        sample_counts={'whole':len(whole), 'top':len(top), 'bottom_left':len(left), 'bottom_right':len(right)})
    return result
