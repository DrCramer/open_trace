"""Bounded, independent JSON kit definitions; no manufacturer profiles included.

Schema v1 uses explicit effective laboratory parameters. Bin ``left_bp`` and
``right_bp`` are nonnegative tolerances, not absolute endpoints. Overlapping
bins are permitted and reported, because ambiguous bin hits must remain visible.
The mathematical neutral bin confidence weight defaults to one. No analytical
cutoff, stutter range or reagent definition is inferred.
"""
from collections.abc import Mapping
import json
import math
from numbers import Real
import re

MAX_JSON_BYTES = 1_048_576
MAX_MARKERS = 128
MAX_BINS_PER_MARKER = 128
MAX_TOTAL_BINS = 4096
MAX_LADDER_LABELS = 32
MAX_RANGES = 64
MAX_TEXT_LENGTH = 128
MAX_BP = 1_000_000


def _object(value, required, optional=(), path='kit'):
    if not isinstance(value, Mapping):
        raise ValueError(f'{path}: object required')
    missing = set(required)-value.keys()
    unknown = value.keys()-set(required)-set(optional)
    if missing:
        raise ValueError(f'{path}: missing fields {", ".join(sorted(missing))}')
    if unknown:
        raise ValueError(f'{path}: unknown fields {", ".join(sorted(map(str, unknown)))}')


def _text(value, path):
    if (not isinstance(value, str) or not 1 <= len(value) <= MAX_TEXT_LENGTH
            or value != value.strip() or any(ord(c) < 32 or ord(c) == 127 for c in value)):
        raise ValueError(f'{path}: nonempty text of at most 128 characters without edge whitespace or controls required')
    return value


def _number(value, path, low=0, high=MAX_BP, *, positive=False):
    try:
        valid = isinstance(value, Real) and not isinstance(value, bool) and math.isfinite(value)
    except (OverflowError, ValueError):
        valid = False
    if not valid or not low <= value <= high or (positive and value == low):
        raise ValueError(f'{path}: finite number in {"(" if positive else "["}{low}, {high}] required')
    return float(value)


def _ranges(value, path, *, ratio):
    if not isinstance(value, list) or len(value) > MAX_RANGES:
        raise ValueError(f'{path}: explicit list of at most {MAX_RANGES} ranges required')
    result = []
    keys = ('min_bp', 'max_bp', 'ratio') if ratio else ('min_bp', 'max_bp')
    for index, row in enumerate(value):
        where = f'{path}[{index}]'
        _object(row, keys, path=where)
        low = _number(row['min_bp'], where+'.min_bp')
        high = _number(row['max_bp'], where+'.max_bp')
        if low > high:
            raise ValueError(f'{where}: min_bp must not exceed max_bp')
        item = {'min_bp': low, 'max_bp': high}
        if ratio:
            item['ratio'] = _number(row['ratio'], where+'.ratio', high=1)
        result.append(item)
    return result


def _parameters(value, path):
    keys = ('ladder_cutoff_ratio', 'cutoff_ratio', 'minus_a_ratio', 'minus_a_min_bp',
            'minus_a_max_bp', 'backward_stutter_ranges', 'forward_stutter_ranges')
    _object(value, keys, ('clear_ranges',), path)
    result = {name: _number(value[name], path+'.'+name, high=1)
              for name in ('ladder_cutoff_ratio', 'cutoff_ratio', 'minus_a_ratio')}
    for name in ('minus_a_min_bp', 'minus_a_max_bp'):
        result[name] = _number(value[name], path+'.'+name)
    if result['minus_a_min_bp'] > result['minus_a_max_bp']:
        raise ValueError(f'{path}: minus_a_min_bp must not exceed minus_a_max_bp')
    for name in ('backward_stutter_ranges', 'forward_stutter_ranges'):
        result[name] = _ranges(value[name], path+'.'+name, ratio=True)
    result['clear_ranges'] = _ranges(value.get('clear_ranges', []), path+'.clear_ranges', ratio=False)
    return result


def _pairs_without_duplicates(pairs):
    result = {}
    for name, value in pairs:
        if name in result:
            raise ValueError(f'Duplicate JSON field: {name}')
        result[name] = value
    return result


def _invalid_constant(value):
    raise ValueError(f'Nonfinite JSON constant: {value}')


def load_kit_definition(source):
    """Validate JSON text/bytes or a mapping; return a new normalized plain dict.

    Raises ValueError on malformed, nonfinite, unknown or excessive inputs.
    Paths are deliberately not opened: callers explicitly read their chosen file
    before passing its JSON. Normalized output can be validated again unchanged.
    Marker/bin order is preserved; ladder labels must follow increasing centers.
    """
    try:
        if isinstance(source, bytes):
            if len(source) > MAX_JSON_BYTES:
                raise ValueError('Kit JSON exceeds 1048576 bytes')
            source = source.decode('utf-8')
        if isinstance(source, str):
            if len(source.encode('utf-8')) > MAX_JSON_BYTES:
                raise ValueError('Kit JSON exceeds 1048576 bytes')
            source = json.loads(source, object_pairs_hook=_pairs_without_duplicates,
                                parse_constant=_invalid_constant)
        else:
            # Bound mapping input too, and reject cycles/non-JSON data before
            # traversing it. This serialization does not execute kit content.
            encoded = json.dumps(source, ensure_ascii=False, allow_nan=False)
            if len(encoded.encode('utf-8')) > MAX_JSON_BYTES:
                raise ValueError('Kit JSON exceeds 1048576 bytes')
    except (UnicodeError, TypeError, OverflowError, RecursionError) as error:
        raise ValueError('Kit must be bounded UTF-8 JSON or a JSON-compatible mapping') from error
    _object(source, ('schema_version', 'name', 'markers'), ('diagnostics',))
    if type(source['schema_version']) is not int or source['schema_version'] != 1:
        raise ValueError('kit.schema_version must be integer 1')
    name = _text(source['name'], 'kit.name')
    rows = source['markers']
    if not isinstance(rows, list) or not 1 <= len(rows) <= MAX_MARKERS:
        raise ValueError('kit.markers: list of 1..128 markers required')
    markers, diagnostics, marker_names, total_bins = [], [], set(), 0
    for index, row in enumerate(rows):
        where = f'kit.markers[{index}]'
        _object(row, ('name', 'channel', 'min_bp', 'max_bp', 'repeat', 'bins',
                      'ladder_labels', 'parameters'), path=where)
        marker_name = _text(row['name'], where+'.name')
        if marker_name.casefold() in marker_names:
            raise ValueError(f'{where}: marker names must be unique ignoring case')
        marker_names.add(marker_name.casefold())
        channel = row['channel']
        if not isinstance(channel, str) or not re.fullmatch(r'DATA[1-9][0-9]{0,5}', channel):
            raise ValueError(f'{where}.channel: canonical DATA number required (e.g. DATA1)')
        low = _number(row['min_bp'], where+'.min_bp', positive=True)
        high = _number(row['max_bp'], where+'.max_bp', positive=True)
        if low >= high:
            raise ValueError(f'{where}: min_bp must be smaller than max_bp')
        repeat = row['repeat']
        if type(repeat) is not int or not 1 <= repeat <= 20:
            raise ValueError(f'{where}.repeat: integer in [1, 20] required')
        bins = row['bins']
        if not isinstance(bins, list) or not 1 <= len(bins) <= MAX_BINS_PER_MARKER:
            raise ValueError(f'{where}.bins: list of 1..128 explicit bins required')
        total_bins += len(bins)
        if total_bins > MAX_TOTAL_BINS:
            raise ValueError('Kit exceeds 4096 total bins')
        normalized_bins, labels = [], {}
        for bin_index, item in enumerate(bins):
            at = f'{where}.bins[{bin_index}]'
            _object(item, ('label', 'center_bp', 'left_bp', 'right_bp'), ('confidence_weight',), at)
            label = _text(item['label'], at+'.label')
            if label.casefold() in labels:
                raise ValueError(f'{at}: bin labels must be unique ignoring case')
            center = _number(item['center_bp'], at+'.center_bp', positive=True)
            if not low <= center <= high:
                raise ValueError(f'{at}: bin center must be inside marker limits')
            left = _number(item['left_bp'], at+'.left_bp')
            right = _number(item['right_bp'], at+'.right_bp')
            if center-left < 0 or center+right > MAX_BP:
                raise ValueError(f'{at}: bin edges must be in [0, 1000000] bp')
            normalized = {'label': label, 'center_bp': center, 'left_bp': left, 'right_bp': right,
                          'confidence_weight': _number(item.get('confidence_weight', 1), at+'.confidence_weight', high=1)}
            labels[label.casefold()] = normalized
            normalized_bins.append(normalized)
        ladder = row['ladder_labels']
        if not isinstance(ladder, list) or len(ladder) > MAX_LADDER_LABELS:
            raise ValueError(f'{where}.ladder_labels: explicit list of at most 32 labels required')
        canonical_ladder, used, centers = [], set(), []
        for label in ladder:
            _text(label, where+'.ladder_labels')
            key = label.casefold()
            if key not in labels or key in used:
                raise ValueError(f'{where}: ladder labels must identify distinct defined bins ignoring case')
            used.add(key)
            canonical_ladder.append(labels[key]['label'])
            centers.append(labels[key]['center_bp'])
        if any(b-a < 1e-6 for a, b in zip(centers, centers[1:])):
            raise ValueError(f'{where}: ladder label centers must increase with gaps >= 0.000001 bp')
        overlap_count, examples = 0, []
        for a_index, a in enumerate(normalized_bins):
            for b in normalized_bins[a_index+1:]:
                if max(a['center_bp']-a['left_bp'], b['center_bp']-b['left_bp']) <= min(a['center_bp']+a['right_bp'], b['center_bp']+b['right_bp']):
                    overlap_count += 1
                    if len(examples) < 16:
                        examples.append([a['label'], b['label']])
        if overlap_count:
            diagnostics.append({'code': 'overlapping_bins', 'marker': marker_name,
                                'pair_count': overlap_count, 'examples': examples,
                                'policy': 'allowed; multiple inclusive bin hits are ambiguous'})
        markers.append({'name': marker_name, 'channel': channel, 'min_bp': low, 'max_bp': high,
                        'repeat': repeat, 'bins': normalized_bins, 'ladder_labels': canonical_ladder,
                        'parameters': _parameters(row['parameters'], where+'.parameters')})
    return {'schema_version': 1, 'name': name, 'markers': markers, 'diagnostics': diagnostics}


validate_kit_definition = load_kit_definition
