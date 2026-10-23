"""Independent full ladder matching and per-bin offsets for explicit kits.

The static ID-X ladder contract is recorded in research/IDX16_ALLELE_CALLING.md
(AL-01/AL-05/AL-07). This is new Python, not a vendor implementation/runtime.
The raw-height ratio objective reuses our independent dynamic programming core;
size-standard height clipping and scan-to-bp rate gates do not apply to bp/bp.
An additional second-best ambiguity guard is our policy. Results are research
outputs, not validated genotypes, PQV values, or probabilities.
"""
from collections.abc import Mapping
import math
from numbers import Real
import re
import statistics

from .kit_definition import load_kit_definition
from .ratio_sizing import _best_paths, SCORE_TOLERANCE

MAX_INPUT_PEAKS = 4096
MAX_MATCH_CANDIDATES = 64
RATIO_FACTOR = 0.6
MIN_MATCH_SCORE = 0.75


def _finite(value, low, high):
    try:
        return (isinstance(value, Real) and not isinstance(value, bool)
                and math.isfinite(value) and low <= value <= high)
    except (ValueError, OverflowError):
        return False


def _flag(peak, name):
    flags = peak.get('flags', {})
    if isinstance(flags, Mapping):
        nested = flags.get(name, False)
        if type(nested) is not bool:
            raise ValueError(f'flags.{name} must be boolean')
    elif isinstance(flags, (list, tuple)) and all(isinstance(flag, str) for flag in flags):
        nested = name in flags
    else:
        raise ValueError('Peak flags must be an object of booleans or a list of flag names')
    direct = peak.get(name, False)
    if type(direct) is not bool:
        raise ValueError(f'Peak {name} must be boolean')
    return direct or nested


def _channels(source):
    if isinstance(source, (list, tuple)):
        if len(source) > MAX_INPUT_PEAKS:
            raise ValueError('At most 4096 total sized peaks are supported')
        grouped = {}
        for peak in source:
            if not isinstance(peak, Mapping) or not isinstance(peak.get('channel'), str):
                raise ValueError('Flat peaks require an explicit channel on every peak')
            grouped.setdefault(peak['channel'], []).append(peak)
        source = grouped
    if not isinstance(source, Mapping) or len(source) > 128:
        raise ValueError('Peaks require a channel mapping or an explicit flat list')
    channels, total = {}, 0
    for channel, peaks in source.items():
        if not isinstance(channel, str) or not re.fullmatch(r'DATA[1-9][0-9]{0,5}', channel):
            raise ValueError('Peak channel must be a canonical DATA number')
        if not isinstance(peaks, (list, tuple)):
            raise ValueError(f'{channel}: explicit peak list required')
        total += len(peaks)
        if total > MAX_INPUT_PEAKS:
            raise ValueError('At most 4096 total sized peaks are supported')
        rows = []
        for index, peak in enumerate(peaks):
            if (not isinstance(peak, Mapping)
                    or not _finite(peak.get('scan'), 0, 100_000_000)
                    or not _finite(peak.get('size_bp'), 1e-300, 1_000_000)
                    or not _finite(peak.get('height'), 5e-324, 1e12)):
                raise ValueError(f'{channel} peak {index}: bounded finite scan, positive size_bp and height required')
            if 'channel' in peak and peak['channel'] != channel:
                raise ValueError('Peak channel disagrees with its channel mapping')
            rows.append({'candidate_index': index, 'scan': float(peak['scan']),
                         'size_bp': float(peak['size_bp']), 'height': float(peak['height']),
                         'spike': _flag(peak, 'spike'), 'pullup': _flag(peak, 'pullup')})
        rows.sort(key=lambda p: p['scan'])
        if any(b['scan']-a['scan'] < 1e-6 or b['size_bp']-a['size_bp'] < 1e-6
               for a, b in zip(rows, rows[1:])):
            raise ValueError(f'{channel}: scan and size order must both increase with gaps >= 0.000001')
        channels[channel] = rows
    return channels


def _match_marker(peaks, marker, low, high, ambiguity_margin):
    result = {'status': 'rejected', 'reasons': [], 'matched': [], 'bin_offsets': {},
              'min_offset_bp': None, 'max_offset_bp': None, 'confidence': None,
              'window': {'min_bp': low, 'max_bp': high, 'inclusive': True},
              'diagnostics': {'external_validation': False, 'score_is_original_sq': False,
                              'ratio_factor': RATIO_FACTOR, 'min_match_score': MIN_MATCH_SCORE,
                              'ambiguity_margin': ambiguity_margin,
                              'ambiguity_policy': 'reject best-minus-runner-up <= margin + 1e-12',
                              'migration_rate_gate': None, 'height_clipping': False}}

    def reject(reason):
        result['reasons'].append(reason)
        return result

    if low > high:
        return reject('Invalid ladder window after applying previous marker offset')
    bins = {row['label']: row for row in marker['bins']}
    labels = marker['ladder_labels']
    if len(labels) < 2:
        return reject('At least two explicit ladder labels are required by this ratio objective')
    expected = [bins[label]['center_bp'] for label in labels]
    window = [p for p in peaks if low <= p['size_bp'] <= high]
    candidates = [p for p in window if not p['spike'] and not p['pullup']]
    result['diagnostics'].update({'window_count': len(window),
                                  'flag_excluded_count': len(window)-len(candidates),
                                  'expected_count': len(expected)})
    if not candidates:
        return reject('No unflagged peaks in the inclusive ladder window')
    cutoff = max(p['height'] for p in candidates)*marker['parameters']['ladder_cutoff_ratio']
    candidates = [p for p in candidates if p['height'] >= cutoff]
    cutoff_count = len(candidates)
    cap_height = None
    if len(candidates) > 2*len(expected):
        cap_height = sorted((p['height'] for p in candidates), reverse=True)[2*len(expected)-1]
        candidates = [p for p in candidates if p['height'] >= cap_height]
    result['diagnostics'].update({'cutoff_height': cutoff, 'after_cutoff_count': cutoff_count,
                                  'top_2n_height': cap_height, 'selected_count': len(candidates),
                                  'candidate_indices': [p['candidate_index'] for p in candidates],
                                  'top_2n_policy': 'height cutoff; all ties retained'})
    if len(candidates) < len(expected):
        return reject('Fewer ladder candidates than explicit expected labels; no partial match emitted')
    if len(candidates) > MAX_MATCH_CANDIDATES:
        return reject('Top-2N cutoff ties exceed the independent 64-candidate computation bound')
    choices, transitions = _best_paths([p['size_bp'] for p in candidates],
                                      [p['height'] for p in candidates], expected, RATIO_FACTOR)
    best, path = choices[0]
    second = choices[1][0] if len(choices) > 1 else None
    gap = best-second if second is not None else None
    result['diagnostics'].update({'best_score': best, 'second_best_score': second,
                                  'ambiguity_gap': gap, 'transitions_checked': transitions,
                                  'complete': len(path) == len(expected)})
    # A rejected best path is kept as diagnostic candidates, never as usable
    # offsets. No lower-ranked path is silently substituted to pass a gate.
    proposed = [dict(candidates[i], label=labels[k], center_bp=expected[k],
                     offset_bp=expected[k]-candidates[i]['size_bp'])
                for k, i in enumerate(path)]
    result['diagnostics']['proposed_matches'] = proposed
    if best+SCORE_TOLERANCE < MIN_MATCH_SCORE:
        return reject('Full ladder assignment is below the ratio score threshold')
    if gap is not None and gap <= ambiguity_margin+SCORE_TOLERANCE:
        return reject('Ambiguous full ladder assignments; offsets withheld by independent review policy')
    heights = [p['height'] for p in proposed]
    threshold = statistics.mean(heights)-statistics.stdev(heights)
    unmatched = [p for i, p in enumerate(candidates) if i not in path]
    penalty_count = sum(p['height'] > threshold for p in unmatched)
    confidence = best*max(0.0, 1-0.25*penalty_count)
    offsets = {p['label']: p['offset_bp'] for p in proposed}
    clear_ranges = marker['parameters']['clear_ranges']
    for row in proposed:
        row['cleared'] = any(span['min_bp'] <= row['size_bp'] <= span['max_bp'] for span in clear_ranges)
        row['confidence'] = confidence
    result.update({'status': 'accepted', 'matched': proposed, 'bin_offsets': offsets,
                   'min_offset_bp': proposed[0]['offset_bp'],
                   'max_offset_bp': proposed[-1]['offset_bp'], 'confidence': confidence})
    result['diagnostics'].update({'unmatched_height_threshold': threshold,
                                  'penalty_unmatched_count': penalty_count,
                                  'confidence_is_probability': False,
                                  'offset_rounding': 'unrounded independent float; no vendor decimal-rounding parity',
                                  'clear_range_policy': 'matched rows marked cleared; offsets retained'})
    return result


def match_ladder(peaks_by_channel, kit, *, size_standard_max_bp, sizing_accepted,
                 ambiguity_margin=0.01, run_offset_state=None):
    """Match a single sized ladder and return per-marker center-minus-observed offsets.

    ``peaks_by_channel`` is {DATA tag: [peak,...]}, or a flat list whose peaks
    each have a channel. Peaks need scan/size_bp/height, optional spike/pullup
    booleans or flags object/list. No flag detector is implied. Explicit accepted
    sizing is required; the size-standard maximum controls the last window on
    each channel. Markers are sorted by minimum size within each channel.

    First window begins at marker.min_bp-7; subsequent windows begin at the
    previous marker maximum minus its accepted final-bin offset. The upper bound
    is raised to the next marker minimum, or is size_standard_max_bp for the last
    marker. With no accepted previous result this single-ladder scope uses zero
    prior offset, reported explicitly. Optional run_offset_state supplies prior
    per-key contributions after withdrawing the current filename. Each later
    window uses the mean of admitted run virtual-max values for the PREVIOUS
    marker plus that marker's fresh local offset, without a local AL gate.
    The first marker remains independent of run offsets. This is a raw-value
    mean, not an equally weighted mean of run mean and current local offset.
    No original PQV gates, controls, edits or marker-subset state are inferred.

    Structural errors return rejected diagnostics. Only accepted marker records
    carry offsets; top-level partial indicates other markers were rejected.
    """
    result = {'status': 'rejected', 'reasons': [], 'markers': {},
              'diagnostics': {'external_validation': False, 'method': 'independent_single_ladder_ratio',
                              'source_contract': 'IDX16_ALLELE_CALLING AL-01/AL-05/AL-07',
                              'run_offset_aggregation': False, 'original_pqv_applied': False}}
    if type(sizing_accepted) is not bool or not sizing_accepted:
        result['reasons'].append('Explicit accepted sizing is required before ladder offsets')
        return result
    if not _finite(size_standard_max_bp, 1e-300, 1_000_000):
        result['reasons'].append('Explicit finite positive size_standard_max_bp <= 1000000 required')
        return result
    if not _finite(ambiguity_margin, 0, 1):
        result['reasons'].append('ambiguity_margin must be finite and in [0, 1]')
        return result
    try:
        definition = load_kit_definition(kit)
        channels = _channels(peaks_by_channel)
        if run_offset_state is not None:
            from .run_offsets import validate_run_offset_state, ladder_window_offset
            run_offset_state = validate_run_offset_state(run_offset_state)
    except (ValueError, TypeError, RecursionError) as error:
        result['reasons'].append(str(error))
        return result
    result['kit_name'] = definition['name']
    result['diagnostics']['kit_diagnostics'] = definition['diagnostics']
    if run_offset_state is not None:
        result['diagnostics'].update(run_offset_aggregation=True, run_window_coupling=True)
    grouped = {}
    for marker in definition['markers']:
        grouped.setdefault(marker['channel'], []).append(marker)
    for channel, markers in grouped.items():
        markers.sort(key=lambda row: (row['min_bp'], row['max_bp'], row['name']))
        previous, previous_offset, previous_local = None, 0.0, None
        for index, marker in enumerate(markers):
            if run_offset_state is not None:
                coupling = (ladder_window_offset(run_offset_state, previous['name'], previous_local)
                            if previous is not None else
                            {'marker': None, 'offset_bp': 0.0, 'run_contributors': [],
                             'local_offset_bp': None, 'value_count': 0,
                             'first_marker_uses_run_offsets': False})
                previous_offset = coupling['offset_bp']
            low = marker['min_bp']-7.0 if previous is None else previous['max_bp']-previous_offset
            high = (max(marker['max_bp']-previous_offset, markers[index+1]['min_bp'])
                    if index+1 < len(markers) else float(size_standard_max_bp))
            row = _match_marker(channels.get(channel, []), marker, low, high, float(ambiguity_margin))
            row['channel'] = channel
            row['diagnostics']['previous_virtual_max_offset_bp'] = previous_offset
            row['diagnostics']['previous_offset_policy'] = 'accepted previous marker from this ladder only; otherwise zero'
            if run_offset_state is not None:
                row['diagnostics']['previous_offset_policy'] = 'admitted run previous-marker virtual-max values plus fresh local value; empty zero'
                row['diagnostics']['previous_offset_sources'] = coupling
            result['markers'][marker['name']] = row
            previous = marker
            previous_offset = row['max_offset_bp'] if row['status'] == 'accepted' else 0.0
            previous_local = row['max_offset_bp'] if row['status'] == 'accepted' else None
    accepted = sum(row['status'] == 'accepted' for row in result['markers'].values())
    if accepted == len(definition['markers']):
        result['status'] = 'accepted'
    elif accepted:
        result['status'] = 'partial'
    if accepted != len(definition['markers']):
        result['reasons'].append('One or more marker ladders lack accepted complete offsets')
    return result
