"""Independent Basic size-standard threshold from a discrete background model.

The inspected wrapper composes x - corrected_background_output(x), clamped
at zero, before taking its median. It does not take the median of the corrected
output itself. This API admits nonnegative integer analyzed signals only and
uses bounded arithmetic without reproducing JVM overflow. No reagent profile,
vendor implementation, target execution or runtime equivalence is included.
"""
import math
from statistics import median

MAX_POINTS = 1_000_000
MAX_VALUE = 100_000_000


def _trunc_div(value, divisor):
    return value//divisor if value >= 0 else -((-value)//divisor)


def _average_stage(signal, width, clip):
    """Finite recurrence, including the distinct even-width indexing rule.

    The first center uses width terms; later centers add i+half and remove
    i-half-1. For an even width this is not an ordinary symmetric box mean.
    Edges keep their input values before clipping to nonnegative values.
    """
    if len(signal) < width:
        return list(signal)
    radius = width//2
    output = list(signal)
    total = sum(signal[:width])
    output[radius] = _trunc_div(total, width)
    for index in range(radius+1, len(signal)-radius):
        total += signal[index+radius]-signal[index-radius-1]
        output[index] = _trunc_div(total, width)
    if clip:
        return [max(0, min(before, after)) for before, after in zip(signal, output)]
    return [max(0, value) for value in output]


def _polynomial_smooth(signal):
    """Quadratic least-squares center estimate on 21 equally spaced points.

    The moment solution has rational weights (329-5*k*k)/3059. Integer
    division truncates toward zero; the ten samples at either edge are kept.
    """
    output = list(signal)
    for index in range(10, len(signal)-10):
        total = 329*signal[index]
        total += sum((329-5*k*k)*(signal[index-k]+signal[index+k]) for k in range(1, 11))
        output[index] = _trunc_div(total, 3059)
    return output


def _directional_track(signal, reverse=False):
    indices = range(len(signal)-1, -1, -1) if reverse else range(len(signal))
    starting = signal[-10:] if reverse else signal[:10]
    level = sum(starting)/10.0
    output = [0]*len(signal)
    for index in indices:
        value = signal[index]
        if value <= int(level):
            level = float(value)
            output[index] = value
            continue
        gap = value-level
        denominator = gap*(gap*gap) if gap > 20 else gap*gap if gap > 10 else gap
        weight = 1.0/denominator if denominator >= 1 else 0.0
        level += weight*gap
        output[index] = int(level+0.5)
    return output


def estimate_standard_autoheight(analyzed):
    """Return threshold, median and control signal for one Basic standard dye.

    Input: 250..1,000,000 nonnegative integers <=100,000,000. The floor30 and
    multiplier25 describe the inspected Basic standard contract. There is no
    ladder floor50 here. Actual user analysis mode/settings remain external.
    control_signal is retained for caller diagnostics; do not confuse it with
    the AP input, which remains the supplied analyzed signal.
    """
    if not isinstance(analyzed, (list, tuple)) or not 250 <= len(analyzed) <= MAX_POINTS:
        raise ValueError('Autoheight requires 250..1000000 analyzed integer samples')
    if any(type(value) is not int or not 0 <= value <= MAX_VALUE for value in analyzed):
        raise ValueError('Autoheight requires nonnegative integer samples <=100000000')
    background = _average_stage(analyzed, 11, False)
    quarter = len(analyzed)//4
    for width in (quarter, 21, 51, 101, 201):
        background = _average_stage(background, width, True)
    background = _polynomial_smooth(background)
    background = _average_stage(background, 21, True)
    forward = _directional_track(background)
    backward = _directional_track(background, True)
    # Keep the two subtractions explicit: the baseline helper returns corrected
    # data, and the wrapper subtracts that result from its original input.
    control = [max(0, value-max(0, value-max(left, right)))
               for value, left, right in zip(analyzed, forward, backward)]
    center = float(median(control))
    threshold = max(30.0, 25.0*center)
    if not math.isfinite(threshold):
        raise ValueError('Autoheight is not finite')
    return {'threshold': threshold, 'median_control': center, 'control_signal': control,
            'parameters': {'floor_rfu': 30.0, 'median_multiplier': 25.0,
                           'first_average': 11, 'quarter_window': quarter,
                           'quarter_window_even': quarter % 2 == 0,
                           'clipped_average_windows': [quarter, 21, 51, 101, 201, 21],
                           'polynomial_window': 21},
            'original_runtime_validated': False, 'external_validation': False}
