"""Independent cubic SG-derivative sizing peak measurements.

Normal finite-signal subset: centered cubic SG derivative, zero-crossing
candidates, explicit slope boundaries, inclusive height/area, raw half-height
width before first-maximum apex snap. Companion/jag/near-base/spike metrics and
legacy-source special branches are deliberately outside this API. Symmetric
pair summation is a numerical design choice, not original JVM LU/convolution.
"""
import math
from numbers import Real

MAX_POINTS = 1_000_000


def _finite(value):
    try:
        return isinstance(value, Real) and not isinstance(value, bool) and math.isfinite(value)
    except (ValueError, OverflowError):
        return False


def cubic_sg_derivative(values, *, window=15):
    """Derivative at unit scan spacing; first/last half-window outputs are zero."""
    if not isinstance(values, (list, tuple)) or not 1 <= len(values) <= MAX_POINTS:
        raise ValueError('Signal must be a nonempty bounded list or tuple')
    if any(type(x) is not int or not -(2**31) <= x < 2**31 for x in values):
        raise ValueError('Signal values must be signed 32-bit integers')
    if type(window) is not int or not 5 <= window <= 101 or window % 2 != 1 or len(values) < window:
        raise ValueError('Cubic SG window must be odd, 5..101, and <= signal length')
    radius = window//2
    s2, s4, s6 = (2*sum(k**power for k in range(1, radius+1)) for power in (2, 4, 6))
    determinant = s2*s6-s4*s4
    weights = [(s6*k-s4*k**3)/determinant for k in range(1, radius+1)]
    derivative = [0.0]*len(values)
    for center in range(radius, len(values)-radius):
        derivative[center] = math.fsum(weight*(values[center+k]-values[center-k])
                                       for k, weight in enumerate(weights, 1))
    return derivative


def _zero(x0, y0, x1, y1):
    if y0 == y1:
        return (x0+x1)/2 if y0 == 0 else None
    return x0-y0*(x1-x0)/(y1-y0)


def _candidates(derivative):
    positive, zero_start, peaks = False, None, []
    for index, value in enumerate(derivative):
        if value > 0:
            positive, zero_start = True, None
        elif value == 0:
            if positive and zero_start is None:
                zero_start = index
        elif positive:
            crossing = ((zero_start+index-1)/2 if zero_start is not None else
                        _zero(index-1, derivative[index-1], index, value))
            peaks.append(crossing)
            positive, zero_start = False, None
    return peaks


def _bounds(derivative, center, tau_begin, tau_end):
    n = len(derivative)
    index = math.ceil(center)-1
    while index >= 0 and 0 <= derivative[index] < tau_begin:
        index -= 1
    if index > 0 and derivative[index] >= tau_begin:
        index -= 1
        while index >= 0 and derivative[index] > tau_begin:
            index -= 1
    begin = 0.0
    if index >= 0 and derivative[index] <= 0:
        begin = _zero(index, derivative[index], index+1, derivative[index+1])
    elif index >= 0 and derivative[index] <= tau_begin:
        begin = _zero(index, derivative[index]-tau_begin, index+1, derivative[index+1]-tau_begin)
    index = math.floor(center)+1
    while index < n and tau_end < derivative[index] <= 0:
        index += 1
    if index < n and derivative[index] <= tau_end:
        index += 1
        while index < n and derivative[index] < tau_end:
            index += 1
    end = float(n-1)
    if index < n and derivative[index] >= 0:
        end = _zero(index-1, derivative[index-1], index, derivative[index])
    elif index < n and derivative[index] >= tau_end:
        end = _zero(index-1, derivative[index-1]-tau_end, index, derivative[index]-tau_end)
    return begin, end


def _interpolate(values, scan):
    left = min(math.floor(scan), len(values)-2)
    if len(values) == 1:
        return float(values[0])
    return values[left]+(scan-left)*(values[left+1]-values[left])


def _half_width(values, lo, hi, center):
    """Ordinary width-return branch with no companion/searchMax/cap."""
    if values[center] <= 0:
        return 0.0
    target = values[center]/2
    left_index, right_index = center-1, center+1
    while left_index >= lo and values[left_index] > target:
        left_index -= 1
    while right_index <= hi and values[right_index] > target:
        right_index += 1
    if left_index < lo:
        left, left_level = float(lo), values[lo]
    else:
        gap = values[left_index+1]-values[left_index]
        left = left_index+(target-values[left_index])/gap if gap > 0 else float(left_index)
        left_level = target if gap > 0 else values[left_index]
    if right_index > hi:
        right, right_level = float(hi), values[hi]
    else:
        gap = values[right_index-1]-values[right_index]
        right = right_index-(target-values[right_index])/gap if gap > 0 else float(right_index)
        right_level = target if gap > 0 else values[right_index]
    # The original normal width-return aligns unequal truncated boundary levels.
    # Its choices use last-above/first-below on the entire opposite half, not
    # an extrapolation or a mirrored half-width.
    if left_level > right_level:
        above = [i for i in range(center, hi+1) if values[i] > left_level]
        below = [i for i in range(center, hi+1) if values[i] < left_level]
        a, b = (above[-1] if above else center), (below[0] if below else hi)
        right = float(a)
        if values[a] > values[b]:
            right += (values[a]-left_level)*(b-a)/(values[a]-values[b])
    elif left_level < right_level:
        below = [i for i in range(lo, center+1) if values[i] < right_level]
        above = [i for i in range(lo, center+1) if values[i] > right_level]
        a, b = (below[-1] if below else lo), (above[0] if above else center)
        left = float(a)
        if values[b] > values[a]:
            left += (right_level-values[a])*(b-a)/(values[b]-values[a])
    return right-left


def detect_sg_peaks(values, *, threshold=100.0, window=15, min_width=2.0,
                    tau_begin=0.0, tau_end=0.0, offscale_intervals=()):
    """Return peak dictionaries and diagnostics; no minimum-distance pruning.

    Width is measured about round-half-up(SG center) before apex snap. Signal
    and intervals stay unchanged. Offscale is only explicit interval overlap;
    a numeric RFU ceiling does not create a flag. Unsupported/malformed peak
    bounds are counted and skipped explicitly. Duplicate snapped scans remain
    visible in diagnostics instead of being silently discarded.
    """
    if not isinstance(values, (list, tuple)):
        raise ValueError('Signal must be a list or tuple')
    if not _finite(threshold) or not 0 < threshold <= 1e12:
        raise ValueError('Threshold must be finite and positive, <= 1e12')
    if not _finite(min_width) or not 0 <= min_width <= 1e6:
        raise ValueError('Minimum width must be finite and in [0, 1000000]')
    if not _finite(tau_begin) or not 0 <= tau_begin <= 1e12 or not _finite(tau_end) or not -1e12 <= tau_end <= 0:
        raise ValueError('Slope thresholds require finite tau_begin>=0 and tau_end<=0')
    if not isinstance(offscale_intervals, (list, tuple)) or len(offscale_intervals) > 10000:
        raise ValueError('Offscale intervals must be a bounded list or tuple')
    for interval in offscale_intervals:
        if (not isinstance(interval, (list, tuple)) or len(interval) != 2
                or any(not _finite(x) for x in interval) or not 0 <= interval[0] <= interval[1] < len(values)):
            raise ValueError('Offscale intervals must lie within the selected signal')
    derivative = cubic_sg_derivative(values, window=window)
    candidates = _candidates(derivative)
    diagnostics = {'sg_candidates': len(candidates), 'invalid_bounds': 0, 'height_rejected': 0,
                   'width_rejected': 0, 'edge_rejected': 0, 'duplicate_snapped_scans': 0,
                   'degree': 3, 'window': window, 'threshold': float(threshold),
                   'companion_metrics': False, 'original_runtime_validated': False}
    peaks = []
    for crossing in candidates:
        begin, end = _bounds(derivative, crossing, tau_begin, tau_end)
        if begin is None or end is None or not 0 <= begin <= crossing <= end < len(values):
            diagnostics['invalid_bounds'] += 1
            continue
        lo, hi, center = math.ceil(begin), math.floor(end), math.floor(crossing+0.5)
        if not lo <= center <= hi:
            diagnostics['invalid_bounds'] += 1
            continue
        height = max(values[lo:hi+1])
        begin_height, end_height = _interpolate(values, begin), _interpolate(values, end)
        if height < threshold:
            diagnostics['height_rejected'] += 1
            continue
        width = _half_width(values, lo, hi, center)
        if not (width >= min_width or begin_height >= threshold or end_height >= threshold):
            diagnostics['width_rejected'] += 1
            continue
        snap = next(i for i in range(lo, hi+1) if values[i] == height)
        if not (int(snap)-int(begin) > 1 and int(end)-int(snap) > 1):
            diagnostics['edge_rejected'] += 1
            continue
        peaks.append({'scan': snap, 'sg_scan': crossing, 'begin': begin, 'end': end,
                      'height': float(height), 'begin_height': begin_height, 'end_height': end_height,
                      'fwhm_scan': width, 'area': sum(values[lo:hi+1]),
                      'offscaled': any(a <= end and b >= begin for a, b in offscale_intervals)})
    peaks.sort(key=lambda p: p['scan'])
    diagnostics['duplicate_snapped_scans'] = len(peaks)-len({p['scan'] for p in peaks})
    diagnostics['accepted_peaks'] = len(peaks)
    return {'peaks': peaks, 'diagnostics': diagnostics}
