"""Independent early-signal and adjacent-peak filters for sizing research.

The mathematical rules are documented from static ID-X bytecode. This bounded
API does not execute or embed vendor code, mutate caller peaks, calculate AP
features, select the tallest N+extra peaks, or evaluate final sizing quality.
Group neighbors refer to a fixed list after the early stage; removing a peak
does not reconnect its neighbors. Malformed connectivity copies are rejected
explicitly rather than reproducing a Java allocation/copy exception.
"""
from collections.abc import Mapping
import math
from numbers import Real

MAX_PEAKS = 4096
MAX_POINTS = 1_000_000


def _finite(value):
    if not isinstance(value, Real) or isinstance(value, bool):
        return False
    try:
        return math.isfinite(value)
    except (OverflowError, ValueError):
        return False


def _ratio(numerator, denominator):
    """IEEE comparison semantics for the two possibly zero morphology ratios."""
    if denominator:
        return numerator/denominator
    if not numerator:
        return math.nan
    return math.copysign(math.inf, numerator)*math.copysign(1.0, denominator)


def filter_standard_peaks(peaks, expected_count, signal, *, group_gap=25):
    """Return copied surviving peaks and explicit filtering diagnostics.

    Peaks must be in strictly increasing scan order and contain scan, height,
    begin/end, begin_height/end_height and fwhm_scan. Optional offscaled defaults
    to False; any other fields (including candidate_index) are preserved.
    Heights are positive; bounds are within a nonempty signed-int signal.
    expected_count is 3..32. Fewer initial candidates are rejected, matching the
    outer matcher guard. Filters may legitimately leave fewer than expected.

    group_gap is an explicit bounded nonnegative scan distance; equality does
    not count as close. The default 25 is an experimental constructor setting,
    not a recovered user analysis method. No original runtime equivalence or
    scientific-accuracy validation is implied by status='passed'.
    """
    diagnostics = {'input_count': len(peaks) if isinstance(peaks, (list, tuple)) else None,
                   'expected_count': expected_count, 'group_gap': group_gap,
                   'early_stage_applied': False, 'early_shape_removed_indices': [],
                   'early_high_removed_indices': [], 'early_prefix_removed_indices': [],
                   'connected_tail_removed_indices': [], 'group_removed_indices': [],
                   'group_events': [], 'original_runtime_validated': False}
    result = {'status': 'rejected', 'peaks': [], 'reasons': [], 'diagnostics': diagnostics}

    def reject(reason):
        result['reasons'].append(reason)
        return result

    if type(expected_count) is not int or not 3 <= expected_count <= 32:
        return reject('expected_count must be an integer in [3, 32]')
    if not _finite(group_gap) or not 0 <= group_gap <= 100_000_000:
        return reject('group_gap must be finite and in [0, 100000000]')
    if not isinstance(signal, (list, tuple)) or not 1 <= len(signal) <= MAX_POINTS:
        return reject('signal must be a nonempty bounded list or tuple')
    if any(type(value) is not int or not -(2**31) <= value < 2**31 for value in signal):
        return reject('signal must contain signed 32-bit integers')
    if not isinstance(peaks, (list, tuple)) or len(peaks) > MAX_PEAKS:
        return reject('peaks must be an explicit list or tuple of at most 4096 peaks')
    checked = []
    for index, source in enumerate(peaks):
        keys = ('scan', 'height', 'begin', 'end', 'begin_height', 'end_height', 'fwhm_scan')
        if not isinstance(source, Mapping) or any(not _finite(source.get(key)) for key in keys):
            return reject(f'Peak {index}: finite numeric morphology fields required')
        if (not 0 <= source['begin'] <= source['scan'] <= source['end'] < len(signal)
                or not 0 < source['height'] <= 1e12 or not 0 <= source['fwhm_scan'] <= MAX_POINTS
                or abs(source['begin_height']) > 1e12 or abs(source['end_height']) > 1e12
                or type(source.get('offscaled', False)) is not bool):
            return reject(f'Peak {index}: invalid bounded morphology or offscaled flag')
        if checked and source['scan'] <= checked[-1][1]['scan']:
            return reject('Peak scans must increase strictly; input is not sorted implicitly')
        checked.append((index, dict(source)))
    if len(checked) < expected_count:
        return reject('Fewer candidates than the full expected standard before prefilters')

    cutoff = (checked[-1][1]['scan']+3*checked[0][1]['scan'])/4
    diagnostics['early_cutoff_scan'] = cutoff
    late = [item for item in checked if item[1]['scan'] >= cutoff]
    diagnostics['late_reference_count'] = len(late)
    survivors = checked
    if len(late) >= 0.75*expected_count:
        diagnostics['early_stage_applied'] = True
        # Python's stable sort preserves input order among equal reference H.
        reference = sorted(late, key=lambda item: -item[1]['height'])[:expected_count]
        mean_height = sum(p['height'] for _, p in reference)/len(reference)
        mean_begin = sum(p['begin_height'] for _, p in reference)/len(reference)
        mean_width = sum(p['fwhm_scan'] for _, p in reference)/len(reference)
        diagnostics['reference_means'] = {'height': mean_height, 'begin_height': mean_begin,
                                           'fwhm_scan': mean_width}
        retained, last_large_scan, last_large = [], 0.0, None
        for original_index, peak in checked:
            if peak['scan'] > cutoff:
                retained.append((original_index, peak))
                continue
            height, begin, end = peak['height'], peak['begin_height'], peak['end_height']
            shape = begin/height > 0.7 or end/height > 0.7 or abs(begin-end)/height > 0.5
            if shape:
                diagnostics['early_shape_removed_indices'].append(original_index)
                update_large = height > 2*mean_height
            elif height > 10*mean_height or (peak['fwhm_scan'] > 2*mean_width and height > 5*mean_height):
                diagnostics['early_high_removed_indices'].append(original_index)
                update_large = True
            else:
                retained.append((original_index, peak))
                update_large = False
            if update_large and peak['scan'] > last_large_scan:
                last_large_scan, last_large = peak['scan'], (original_index, peak)
        survivors = []
        for item in retained:
            if item[1]['scan'] > last_large_scan:
                survivors.append(item)
            else:
                diagnostics['early_prefix_removed_indices'].append(item[0])
        diagnostics['last_large_early_scan'] = last_large_scan
        if survivors and last_large is not None:
            original_index, first = survivors[0]
            if (_ratio(first['height']-first['begin_height'], first['height']-first['end_height']) > 0.5
                    and _ratio(first['begin_height'], mean_begin) > 5):
                previous_end, current_begin = last_large[1]['end'], first['begin']
                if previous_end == current_begin:
                    connected = True
                else:
                    # This is the explicit observed int-copy interval, not an
                    # interpolated valley or reconstructed nearest-neighbor span.
                    start, count = int(previous_end), int(current_begin-previous_end+1)
                    if count < 0 or start < 0 or start+count > len(signal):
                        diagnostics['connectivity_error'] = {'start': start, 'count': count,
                                                              'previous_index': last_large[0],
                                                              'current_index': original_index}
                        return reject('Malformed connected-tail signal interval; filtering withheld')
                    connected = all(value >= first['begin_height'] for value in signal[start:start+count])
                if connected:
                    diagnostics['connected_tail_removed_indices'].append(original_index)
                    survivors = survivors[1:]
    else:
        diagnostics['early_stage_skip_reason'] = 'Fewer than 0.75*expected_count late reference peaks'
    diagnostics['after_early_count'] = len(survivors)

    # Fixed neighbors within this stage's input list. The examined set contains
    # both already visited dominant peaks and neighbors chosen for removal.
    right_shape = [(p['end_height']/p['height'] > 0.7
                    or (p['end_height']-p['begin_height'])/p['height'] > 0.5) for _, p in survivors]
    left_shape = [(p['begin_height']/p['height'] > 0.7
                   or (p['begin_height']-p['end_height'])/p['height'] > 0.5) for _, p in survivors]
    examined, removed = set(), set()
    remaining = len(survivors)
    while remaining > expected_count and len(examined) < len(survivors):
        dominant = max((i for i in range(len(survivors)) if i not in examined),
                       key=lambda i: survivors[i][1]['height'])
        original_index, peak = survivors[dominant]
        chosen, side = None, None
        for neighbor, direction, flags in ((dominant-1, 'left', right_shape),
                                            (dominant+1, 'right', left_shape)):
            if not 0 <= neighbor < len(survivors) or neighbor in examined:
                continue
            other = survivors[neighbor][1]
            shape_smaller = flags[neighbor] and other['height'] < peak['height']
            lower_ratio = other['height']/peak['height'] < 0.6
            if (abs(other['scan']-peak['scan']) < group_gap and (shape_smaller or lower_ratio)
                    and not peak.get('offscaled', False)):
                chosen, side = neighbor, direction
                break
        if chosen is not None:
            removed.add(chosen)
            examined.add(chosen)
            remaining -= 1
            diagnostics['group_removed_indices'].append(survivors[chosen][0])
            diagnostics['group_events'].append({'dominant_index': original_index,
                                                 'removed_index': survivors[chosen][0], 'side': side})
        examined.add(dominant)
    output = [dict(peak) for index, (_, peak) in enumerate(survivors) if index not in removed]
    diagnostics.update({'output_count': len(output), 'below_expected_after_filtering': len(output) < expected_count,
                        'group_examined_count': len(examined)})
    result.update(status='passed', peaks=output)
    return result
