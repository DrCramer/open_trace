"""Independent mathematical trace preparation, available as an explicit API.

The baseline is the mean of close_spike(open_window(x)) and
open_window(close_spike(x)), using flat centered, clipped-support morphology.
An optional clipped moving average smooths the baseline. Subtraction is clipped
at zero and rounded half upward. Integer triangular smoothing keeps endpoints.

This implements the documented mathematical specification independently. It is
not a complete GeneMapper pipeline or runtime-equivalence claim. Odd windows
and bounded signed-int input are explicit API constraints; integer sums do not
emulate Java overflow. Existing UI/CLI analysis does not enable this implicitly.
"""
from collections import deque
import math

MAX_POINTS = 1_000_000
MAX_WINDOW = 100_001


def _trace(values):
    if not isinstance(values, (list, tuple)) or not 1 <= len(values) <= MAX_POINTS:
        raise ValueError("Trace must be a nonempty bounded list or tuple")
    if any(type(x) is not int or not -(2**31) <= x < 2**31 for x in values):
        raise ValueError("Trace values must be signed 32-bit integers")
    return list(values)


def _window(width):
    if type(width) is not int or not 1 <= width <= MAX_WINDOW or width % 2 != 1:
        raise ValueError("Window must be an odd integer in [1, 100001]")
    return width // 2


def _extrema(values, radius, maximum):
    """Linear-time extrema; every output uses only available centered samples."""
    queue, output, inserted = deque(), [], 0
    n = len(values)
    for center in range(n):
        stop = min(n, center+radius+1)
        while inserted < stop:
            value = values[inserted]
            while queue and ((values[queue[-1]] <= value) if maximum else (values[queue[-1]] >= value)):
                queue.pop()
            queue.append(inserted)
            inserted += 1
        start = max(0, center-radius)
        while queue[0] < start:
            queue.popleft()
        output.append(values[queue[0]])
    return output


def _average(values, radius):
    output, left, right, total = [], 0, 0, 0.0
    for center in range(len(values)):
        stop = min(len(values), center+radius+1)
        while right < stop:
            total += values[right]
            right += 1
        start = max(0, center-radius)
        while left < start:
            total -= values[left]
            left += 1
        output.append(total/(right-left))
    return output


def estimate_baseline(values, *, window, spike_window, smoothing_window=1):
    """Return a floating baseline with explicit odd widths and clipped edges."""
    signal = _trace(values)
    broad, narrow, smooth = map(_window, (window, spike_window, smoothing_window))

    def opening(trace, radius):
        return _extrema(_extrema(trace, radius, False), radius, True)

    def closing(trace, radius):
        return _extrema(_extrema(trace, radius, True), radius, False)

    lower = closing(opening(signal, broad), narrow)
    upper = opening(closing(signal, narrow), broad)
    result = [(a+b)/2 for a, b in zip(lower, upper)]
    return _average(result, smooth) if smooth else result


def smooth_trace(values, *, passes):
    """Two adjacent averages per pass, arithmetic floor, unchanged endpoints.

    For length >=3 each interior sample becomes
    floor((floor((left+center)/2)+floor((center+right)/2))/2).
    Both adjacent sums use the same pass's input. Lengths 1/2 stay unchanged.
    """
    signal = _trace(values)
    if type(passes) is not int or not 0 <= passes <= 32:
        raise ValueError("Smoothing passes must be an integer in [0, 32]")
    for _ in range(passes):
        if len(signal) < 3:
            break
        adjacent = [(a+b)//2 for a, b in zip(signal, signal[1:])]
        signal = [signal[0]]+[(a+b)//2 for a, b in zip(adjacent, adjacent[1:])]+[signal[-1]]
    return signal


def prepare_trace(values, *, window, spike_window, smoothing_window=1, smooth_passes=0):
    """Return baseline, corrected integer signal and optionally smoothed signal.

    No matrix transformation, normalization, peak detector or sizing is applied.
    No input array is modified. The baseline estimate may be negative.
    """
    signal = _trace(values)
    baseline = estimate_baseline(signal, window=window, spike_window=spike_window,
                                 smoothing_window=smoothing_window)
    corrected = [math.floor(max(0.0, x-b)+0.5) for x, b in zip(signal, baseline)]
    # A signed-int input can yield a nonnegative residual larger than INT_MAX.
    # Reject that unsupported case explicitly instead of emulating Java overflow.
    if any(x >= 2**31 for x in corrected):
        raise ValueError("Baseline-corrected trace exceeds signed 32-bit range")
    prepared = smooth_trace(corrected, passes=smooth_passes)
    return {"baseline": baseline, "corrected": corrected, "signal": prepared,
            "parameters": {"window": window, "spike_window": spike_window,
                           "smoothing_window": smoothing_window, "smooth_passes": smooth_passes,
                           "edge_policy": "clipped centered support", "rounding": "nonnegative half up",
                           "original_runtime_validated": False}}
