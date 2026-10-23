"""Independent size-standard coordinate projection before matching.

Input features are measured on the local AP signal. The inspected wrapper
truncates scan/begin/end to integers, adds OffsetPt, marks interval offscale,
then its matching caller sorts by scan. Height, interpolated boundary heights,
width and area are retained. This module does not run a detector or crop traces.
Absolute output is not suitable for local signal indexing without an explicit
coordinate adapter. The current sizing pipeline uses offset0 only.
"""
from collections.abc import Mapping
import math
from numbers import Real


def _finite(value):
    try:
        return isinstance(value, Real) and not isinstance(value, bool) and math.isfinite(value)
    except (ValueError, OverflowError):
        return False


def wrap_standard_peaks(peaks, *, signal_length, offset=0, offscale_intervals=()):
    """Return copied, sorted projected peaks and diagnostics.

    Local coordinates must satisfy0<=begin<=scan<=end<signal_length. Optional
    offscaled flags are retained by OR. Offscale intervals are absolute and
    may extend beyond the selected trace; inclusive overlap is checked only
    after projection. Source fractions are retained in wrapper_source_local.
    Duplicate projected scans remain visible for downstream validation.
    """
    if type(signal_length) is not int or not 1 <= signal_length <= 1_000_000:
        raise ValueError('signal_length must be an integer in [1, 1000000]')
    if type(offset) is not int or not 0 <= offset <= 2**31-signal_length:
        raise ValueError('offset must keep projected coordinates within nonnegative signed32 range')
    if not isinstance(peaks, (list, tuple)) or len(peaks) > 4096:
        raise ValueError('peaks must be a list or tuple of at most4096 mappings')
    if not isinstance(offscale_intervals, (list, tuple)) or len(offscale_intervals) > 10000:
        raise ValueError('offscale_intervals must be a bounded list or tuple')
    for interval in offscale_intervals:
        if (not isinstance(interval, (list, tuple)) or len(interval) != 2
                or any(not _finite(x) for x in interval) or not 0 <= interval[0] <= interval[1] <= 2**31-1):
            raise ValueError('offscale intervals require finite ordered nonnegative signed32 coordinates')
    output, changed, new_offscale = [], 0, 0
    for peak in peaks:
        if not isinstance(peak, Mapping):
            raise ValueError('Each peak must be a coordinate mapping')
        if any(not _finite(peak.get(key)) for key in ('begin', 'scan', 'end')):
            raise ValueError('Each peak requires finite begin, scan and end')
        if not 0 <= peak['begin'] <= peak['scan'] <= peak['end'] < signal_length:
            raise ValueError('Peak coordinates must lie within the local signal')
        if type(peak.get('offscaled', False)) is not bool:
            raise ValueError('offscaled must be bool when supplied')
        if 'wrapper_source_local' in peak:
            raise ValueError('Peaks must be projected once; wrapper_source_local is reserved')
        item = dict(peak)
        item['wrapper_source_local'] = {key: peak[key] for key in ('begin', 'scan', 'end')}
        for key in ('begin', 'scan', 'end'):
            changed += peak[key] != int(peak[key])
            item[key] = int(peak[key])+offset
        previous = peak.get('offscaled', False)
        item['offscaled'] = previous or any(a <= item['end'] and b >= item['begin'] for a, b in offscale_intervals)
        new_offscale += item['offscaled'] and not previous
        output.append(item)
    output.sort(key=lambda p: p['scan'])
    return {'peaks': output, 'diagnostics': {'input_count': len(peaks), 'output_count': len(output),
            'signal_length': signal_length, 'offset': offset, 'fractional_coordinates_truncated': changed,
            'new_offscaled_peaks': new_offscale,
            'duplicate_projected_scans': len(output)-len({p['scan'] for p in output}),
            'offscale_interval_count': len(offscale_intervals),
            'output_coordinate_scope': 'local' if offset == 0 else 'absolute',
            'original_runtime_validated': False}}
