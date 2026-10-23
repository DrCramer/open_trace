"""Independent, bounded, research-only STR sample calling.

The equations and boundary conditions are recorded in
research/IDX16_ALLELE_CALLING.md (AL-01 through AL-05). No original code or
runtime is loaded. All kit/filter constants and all ladder offsets are inputs.
This module emits reviewable hypotheses, never a validated genotype.
"""
from collections.abc import Mapping
from copy import deepcopy
import json
import math
from numbers import Real
import re

from .kit_definition import load_kit_definition

MAX_INPUT_PEAKS = 4096
MAX_MARKER_PEAKS = 512
MAX_PAIR_RANGE_CHECKS = 8_000_000
MAX_ABSOLUTE_OFFSET_BP = 1_000_000


def _number(value):
    return (isinstance(value, Real) and not isinstance(value, bool)
            and math.isfinite(value))


def _prepare_peaks(peaks):
    if not isinstance(peaks, (list, tuple)) or len(peaks) > MAX_INPUT_PEAKS:
        raise ValueError(f'peaks must be a list of at most {MAX_INPUT_PEAKS} records')
    prepared = []
    for index, peak in enumerate(peaks):
        if not isinstance(peak, Mapping):
            raise ValueError(f'peak {index} must be an object')
        channel = peak.get('channel')
        if not isinstance(channel, str) or not re.fullmatch(r'DATA[1-9][0-9]{0,5}', channel):
            raise ValueError(f'peak {index} needs a canonical DATA channel')
        for key in ('scan', 'size_bp', 'height'):
            if not _number(peak.get(key)):
                raise ValueError(f'peak {index} needs finite {key}')
        if not (0 <= peak['scan'] <= 100_000_000 and 0 < peak['size_bp'] <= 1_000_000
                and 0 < peak['height'] <= 1e12):
            raise ValueError(f'peak {index} needs bounded scan and positive bounded size/height')
        flags = peak.get('flags', [])
        if isinstance(flags, Mapping):
            if any(not isinstance(name, str) or not isinstance(value, bool)
                   for name, value in flags.items()):
                raise ValueError(f'peak {index} flag map must contain string/boolean pairs')
            flags = [name for name, value in flags.items() if value]
        if not isinstance(flags, (list, tuple)) or any(not isinstance(f, str) for f in flags):
            raise ValueError(f'peak {index} flags must be strings or a boolean flag map')
        flags = list(dict.fromkeys(flags))
        for flag in ('spike', 'pullup'):
            if flag in peak and not isinstance(peak[flag], bool):
                raise ValueError(f'peak {index} {flag} must be boolean')
            if peak.get(flag) is True and flag not in flags:
                flags.append(flag)
        prepared.append({'peak_index': index, 'channel': channel,
                         'scan': float(peak['scan']), 'size_bp': float(peak['size_bp']),
                         'height': float(peak['height']), 'flags': flags})
    # This independent API normalizes order explicitly. The original picker
    # consumes its caller's order without sorting. Ties preserve input identity.
    return sorted(prepared, key=lambda p: (p['size_bp'], p['peak_index']))


def _reference_offsets(marker):
    # ID-X 'continue analysis of all run folders': without a passing ladder the
    # reference bin locations from the panel are used unshifted.
    by_bin = {b['label']: {'offset_bp': 0.0, 'origin': 'reference_bins', 'source_bin': b['label'], 'derived': True}
              for b in marker['bins']}
    record = {'bin_offsets': {b['label']: 0.0 for b in marker['bins']}, 'min_offset_bp': 0.0, 'max_offset_bp': 0.0}
    return record, (marker['min_bp'], marker['max_bp']), {
        'policy': 'reference_bins', 'bin_order': 'kit_declared_order', 'by_bin': by_bin,
        'min_offset_bp': 0.0, 'max_offset_bp': 0.0, 'reference_bins': True}


def _marker_offsets(marker, ladder_offsets, missing_offset_policy):
    if missing_offset_policy == 'reference_bins':
        record = ladder_offsets.get('markers', {}).get(marker['name']) if isinstance(ladder_offsets, Mapping) else None
        if (not isinstance(ladder_offsets, Mapping) or ladder_offsets.get('status') not in ('accepted', 'partial')
                or not isinstance(record, Mapping) or record.get('status') != 'accepted'):
            return _reference_offsets(marker)
        missing_offset_policy = 'forward_fill'
    if not isinstance(ladder_offsets, Mapping):
        raise ValueError('ladder_offsets must be an explicit ladder result object')
    if ladder_offsets.get('status') not in ('accepted', 'partial'):
        raise ValueError('ladder result must be accepted or partially accepted')
    markers = ladder_offsets.get('markers')
    if not isinstance(markers, Mapping):
        raise ValueError('ladder_offsets.markers must be an object')
    record = markers.get(marker['name'])
    if not isinstance(record, Mapping) or record.get('status') != 'accepted':
        raise ValueError('missing or unaccepted ladder offsets')
    offsets = record.get('bin_offsets')
    if not isinstance(offsets, Mapping):
        raise ValueError('bin_offsets must be an object')
    labels = {bin_['label'] for bin_ in marker['bins']}
    if not offsets or any(label not in labels for label in offsets):
        raise ValueError('bin_offsets need direct offsets with exact canonical bin-label keys')
    for bound in ('min_offset_bp', 'max_offset_bp'):
        if not _number(record.get(bound)) or abs(record[bound]) > MAX_ABSOLUTE_OFFSET_BP:
            raise ValueError(f'missing, nonfinite or excessive {bound}')
    for label, offset in offsets.items():
        if not _number(offset) or abs(offset) > MAX_ABSOLUTE_OFFSET_BP:
            raise ValueError(f'nonfinite or excessive offset for bin {label}')
    resolved, by_bin, previous_label = {}, {}, None
    for bin_ in marker['bins']:
        label = bin_['label']
        if label in offsets:
            delta, origin, source_bin = offsets[label], 'direct_ladder', label
        elif missing_offset_policy == 'reject':
            raise ValueError(f'missing offset for bin {label}')
        elif previous_label is None:
            # This explicit policy uses the accepted virtual minimum; unlike
            # the original fallback, it never synthesizes a zero correction.
            delta, origin, source_bin = record['min_offset_bp'], 'virtual_min', None
        else:
            delta, origin, source_bin = resolved[previous_label], 'forward_fill', previous_label
        resolved[label] = delta
        by_bin[label] = {'offset_bp': delta, 'origin': origin, 'source_bin': source_bin,
                         'derived': origin != 'direct_ladder'}
        previous_label = label
    low = marker['min_bp'] - record['min_offset_bp']
    high = marker['max_bp'] - record['max_offset_bp']
    if not _number(low) or not _number(high) or low > high:
        raise ValueError('ladder offsets produce an invalid marker window')
    if any(not _number(bin_['center_bp'] + resolved[bin_['label']])
           for bin_ in marker['bins']):
        raise ValueError('ladder offset arithmetic is nonfinite')
    resolved_record = {'bin_offsets': resolved, 'min_offset_bp': record['min_offset_bp'],
                       'max_offset_bp': record['max_offset_bp']}
    metadata = {'policy': missing_offset_policy, 'bin_order': 'kit_declared_order',
                'by_bin': by_bin, 'min_offset_bp': record['min_offset_bp'],
                'max_offset_bp': record['max_offset_bp']}
    return resolved_record, (low, high), metadata


def _label(peak, bins, offsets, *, stutter=False):
    hits = []
    for bin_ in bins:
        corrected = peak['size_bp'] + offsets[bin_['label']]
        if not math.isfinite(corrected):
            raise ValueError('corrected peak size is nonfinite')
        if bin_['center_bp'] - bin_['left_bp'] <= corrected <= bin_['center_bp'] + bin_['right_bp']:
            hits.append(bin_)
    unique = len(hits) == 1
    call = {key: deepcopy(peak[key]) for key in
            ('peak_index', 'scan', 'size_bp', 'height', 'flags')}
    call.update(label=hits[0]['label'] if unique else 'OL',
                bin_hits=[b['label'] for b in hits], review_required=True,
                kind='stutter' if stutter else 'selected')
    if not stutter:
        # A coefficient is retained for traceability, never called a probability.
        call['off_ladder'] = not unique
        call['confidence_weight'] = hits[0]['confidence_weight'] if unique else 1.0
    return call


def _pick_marker(marker, candidates, offset_record, window, offset_metadata):
    parameters = marker['parameters']
    result = {'name': marker['name'], 'channel': marker['channel'],
              'status': 'tentative', 'reasons': [], 'window_bp': list(window),
              'calls': [], 'stutter_calls': [], 'peak_review': [], 'events': []}
    result['offsets'] = offset_metadata
    for label, metadata in offset_metadata['by_bin'].items():
        if metadata['derived']:
            result['events'].append({'reason': 'derived_bin_offset', 'bin_label': label,
                                     **metadata})
    reviews = {}
    active = []
    for peak in candidates:
        review = {**deepcopy(peak), 'state': 'candidate', 'reasons': []}
        reviews[peak['peak_index']] = review
        result['peak_review'].append(review)
        artifacts = [f for f in ('spike', 'pullup') if f in peak['flags']]
        if artifacts:
            review['state'] = 'excluded_artifact'
            review['reasons'] = artifacts
            result['events'].append({'peak_index': peak['peak_index'],
                                     'reason': 'excluded_artifact', 'flags': artifacts})
        else:
            active.append(peak)
    if not active:
        return result
    threshold = max(p['height'] for p in active) * parameters['cutoff_ratio']
    result['cutoff_height'] = threshold
    eligible = [p['height'] > threshold for p in active]
    stutter_indices = []

    def exclude(index, reason, **details):
        eligible[index] = False
        peak_index = active[index]['peak_index']
        reviews[peak_index]['state'] = reason
        reviews[peak_index]['reasons'].append(reason)
        result['events'].append({'peak_index': peak_index, 'reason': reason, **details})

    for i, selected in enumerate(eligible):
        if not selected:
            exclude(i, 'below_or_equal_cutoff', threshold=threshold)
    for i in range(len(active) - 1):
        if not eligible[i]:
            continue
        for j in range(i + 1, len(active)):
            if not eligible[j]:
                continue
            distance = active[j]['size_bp'] - active[i]['size_bp']
            ratio = active[i]['height'] / active[j]['height']
            if (parameters['minus_a_min_bp'] <= distance <= parameters['minus_a_max_bp']
                    and ratio < parameters['minus_a_ratio']):
                exclude(i, 'minus_a', parent_peak_index=active[j]['peak_index'],
                        distance_bp=distance, height_ratio=ratio)
                break

    def remove_stutter(i, j, ranges, forward):
        distance = active[j]['size_bp'] - active[i]['size_bp']
        child, parent = (j, i) if forward else (i, j)
        ratio = active[child]['height'] / active[parent]['height']
        for range_index, range_ in enumerate(ranges):
            low, high = sorted((range_['min_bp'], range_['max_bp']))
            if low <= distance <= high and ratio < range_['ratio']:
                reason = 'forward_stutter' if forward else 'backward_stutter'
                exclude(child, reason, parent_peak_index=active[parent]['peak_index'],
                        distance_bp=distance, height_ratio=ratio, range_index=range_index)
                stutter_indices.append(child)
                return True
        return False

    # Mutating eligibility in this order is part of the documented contract.
    for i in range(len(active) - 1):
        if not eligible[i]:
            continue
        for j in range(i + 1, len(active)):
            if eligible[j] and remove_stutter(i, j, parameters['backward_stutter_ranges'], False):
                break
    for j in range(1, len(active)):
        if not eligible[j]:
            continue
        for i in range(j):
            if eligible[i] and remove_stutter(i, j, parameters['forward_stutter_ranges'], True):
                break

    def clear_or_label(i, stutter):
        peak = active[i]
        review = reviews[peak['peak_index']]
        for range_ in parameters['clear_ranges']:
            if range_['min_bp'] <= peak['size_bp'] <= range_['max_bp']:
                review['state'] = 'clear_range'
                review['reasons'].append('clear_range')
                result['events'].append({'peak_index': peak['peak_index'], 'reason': 'clear_range'})
                return
        call = _label(peak, marker['bins'], offset_record['bin_offsets'], stutter=stutter)
        if len(call['bin_hits']) == 1:
            metadata = offset_metadata['by_bin'][call['bin_hits'][0]]
            call.update(offset_bp=metadata['offset_bp'], offset_origin=metadata['origin'],
                        offset_derived=metadata['derived'])
            if metadata['derived'] and 'derived_bin_offset' not in call['flags']:
                call['flags'].append('derived_bin_offset')
        if stutter:
            result['stutter_calls'].append(call)
            return
        review['state'] = 'selected'
        call['stutter_fraction_annotation'] = None
        for neighbor in active:
            delta = peak['size_bp'] - neighbor['size_bp']
            if (marker['repeat'] - 0.5 < delta < marker['repeat'] + 0.5
                    and neighbor['height'] / peak['height'] < 0.2):
                call['stutter_fraction_annotation'] = neighbor['height'] / peak['height']
                break
        if call['off_ladder']:
            reason = 'multiple_bin_hits' if len(call['bin_hits']) > 1 else 'no_bin_hit'
            review['reasons'].append(reason)
            result['events'].append({'peak_index': peak['peak_index'], 'reason': reason,
                                     'bin_hits': call['bin_hits']})
        result['calls'].append(call)

    for i, selected in enumerate(eligible):
        if selected:
            clear_or_label(i, False)
    for i in stutter_indices:
        clear_or_label(i, True)
    return result


def call_sample(peaks, kit, ladder_offsets, *, sizing_accepted, source_metadata=None,
                missing_offset_policy='reject',spike_adjustments=None):
    """Call explicit sized peaks using a v1 kit and explicit accepted offsets.

    ``peaks`` is a flat list of channel/scan/size_bp/height records. Optional
    flags are a list of strings or a string/boolean mapping (``spike`` and
    ``pullup`` exclude a candidate), or explicit boolean fields with those names.
    Flags are supplied by the caller; this module does not infer morphology.
    Optional explicit spike_adjustments applies the fresh sample height rule
    after each marker's selection, then the per-channel marker envelope.
    Reviews retain entry flags. Cleared peaks are not inserted into that
    marker again; later overlapping markers see the updated current flags.
    Marker metrics retain the phase at which they were calculated. The kit
    declared marker order is our policy, not an original marker-order oracle.
    Duplicate positions remain
    distinct peaks; input list index supplies cross-marker identity.

    Kit stutter ranges are already resolved effective ranges. Empty lists
    disable a direction; original method/marker fallback is not guessed.
    ``ladder_offsets`` has the shape returned by ``match_ladder``: markers[name]
    needs status='accepted', bin_offsets, min_offset_bp and max_offset_bp.
    Delta means bin center minus measured ladder size. Window bounds subtract
    the corresponding delta; bin assignment adds the per-bin delta to size.
    Missing offsets reject a marker by default. Explicit policy 'forward_fill'
    uses the kit's declared bin order, first missing bin's accepted virtual
    minimum, then the previous resolved offset. There is no interpolation or
    implicit zero. Policy 'reference_bins' behaves as 'forward_fill' where the
    ladder gives offsets and uses unshifted panel bins where it gives none.
    All derivations carry metadata, events and call flags.

    Invalid input returns rejected diagnostics. The only successful status is
    tentative, including cases with no selected peak. Sizing/offset acceptance
    is caller-provided state, not independent calibration validation. Metadata
    is copied as supplied and is not a provenance attestation. Inputs are never
    mutated, and no selected call is suitable for identity interpretation.
    """
    result = {'status': 'rejected', 'reasons': [], 'markers': [],
              'research_only': True, 'external_validation': False,
              'original_runtime_validated': False,
              'source_metadata': None, 'unassigned_peak_indices': [],
              'diagnostics': {'method': 'explicit_kit_ordered_sample_filters',
                              'input_order': 'stable_size_bp_then_input_index',
                              'offset_missing_policy': missing_offset_policy,
                              'stutter_ranges': 'explicit_effective_ranges'}}
    try:
        definition = load_kit_definition(kit)
        if missing_offset_policy not in ('reject', 'forward_fill', 'reference_bins'):
            raise ValueError('missing_offset_policy must be reject, forward_fill or reference_bins')
        if sizing_accepted is not True:
            raise ValueError('sizing must be explicitly accepted before sample calling')
        if source_metadata is not None and not isinstance(source_metadata, Mapping):
            raise ValueError('source_metadata must be an object or None')
        if source_metadata is not None:
            metadata_json = json.dumps(dict(source_metadata), allow_nan=False)
            if len(metadata_json.encode('utf-8')) > 1_048_576:
                raise ValueError('source_metadata exceeds 1048576 bytes')
        prepared = _prepare_peaks(peaks)
        spike_state=None
        if spike_adjustments is not None:
            from .spike_adjustment import SpikeState
            spike_state=SpikeState(prepared,spike_adjustments)
    except (TypeError, ValueError, OverflowError, RecursionError) as error:
        result['reasons'].append(str(error))
        return result
    result['source_metadata'] = deepcopy(dict(source_metadata)) if source_metadata is not None else None
    result['kit_name'] = definition['name']
    pair_range_checks = 0
    assigned = set()
    for marker in definition['markers']:
        rejected = {'name': marker['name'], 'channel': marker['channel'],
                    'status': 'rejected', 'reasons': [], 'calls': [],
                    'stutter_calls': [], 'peak_review': [], 'events': []}
        try:
            offset_record, window, offset_metadata = _marker_offsets(marker, ladder_offsets, missing_offset_policy)
            candidates = [p for p in prepared if p['channel'] == marker['channel']
                          and window[0] <= p['size_bp'] <= window[1]]
            if len(candidates) > MAX_MARKER_PEAKS:
                raise ValueError(f'marker exceeds {MAX_MARKER_PEAKS} candidate peaks')
            ranges = len(marker['parameters']['backward_stutter_ranges']) + len(marker['parameters']['forward_stutter_ranges'])
            pair_range_checks += (len(candidates) * (len(candidates) - 1) // 2 * (1 + ranges)
                                  + len(candidates) ** 2)
            if pair_range_checks > MAX_PAIR_RANGE_CHECKS:
                raise ValueError('sample exceeds bounded pair/range work budget')
            marker_result = _pick_marker(marker, candidates, offset_record, window, offset_metadata)
            if spike_state is not None:spike_state.after_sample_marker(marker_result)
            assigned.update(p['peak_index'] for p in candidates)
            result['markers'].append(marker_result)
        except (TypeError, ValueError, OverflowError) as error:
            rejected['reasons'].append(str(error))
            result['markers'].append(rejected)
    # Original object-reference overlap becomes explicit input-index identity.
    by_peak = {}
    for marker in result['markers']:
        for call in marker['calls']:
            by_peak.setdefault(call['peak_index'], []).append((marker, call))
    for peak_index, calls in by_peak.items():
        if len(calls) > 1:
            names = [marker['name'] for marker, _ in calls]
            for marker, call in calls:
                if 'cross_marker_overlap' not in call['flags']:
                    call['flags'].append('cross_marker_overlap')
                marker['events'].append({'peak_index': peak_index,
                                         'reason': 'cross_marker_overlap', 'markers': names})
    result['unassigned_peak_indices'] = sorted(p['peak_index'] for p in prepared
                                                if p['peak_index'] not in assigned)
    rejected_names = [m['name'] for m in result['markers'] if m['status'] == 'rejected']
    if rejected_names:
        result['reasons'].append('rejected markers: ' + ', '.join(rejected_names))
    if result['markers'] and not rejected_names:
        result['status'] = 'tentative'
    elif len(rejected_names) < len(result['markers']):
        result['status'] = 'partial'
    result['diagnostics']['pair_range_work_upper_bound'] = pair_range_checks
    if spike_state is not None:result['spike_adjustments']=spike_state.finish_sample(result['markers'])
    return result
