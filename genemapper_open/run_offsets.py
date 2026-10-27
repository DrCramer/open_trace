"""Explicit run contributions, per-key means, and fresh reanalysis replacement.

Static contracts: IDX16_ALLELE_CALLING AL-05/AL-07. Numeric AL codes are supplied
by the caller, never inferred from matching or manufactured here. Structured
keys avoid the original concatenated-key collisions. Incoming offsets use the
observed nine-decimal Math.round storage rule and means use sequential double
addition. This does not establish complete original runtime equivalence.
"""
from collections.abc import Mapping
from .fastcopy import fastcopy
import math

from .kit_definition import load_kit_definition


ALLOWED_CODES = (0.0, -2.0, 0.5)
_KINDS = ('bin', 'virtual_min', 'virtual_max')


def _name(value, name, limit=256, *, ascii_only=False):
    if (not isinstance(value, str) or not 1 <= len(value) <= limit
            or (ascii_only and not value.isascii())):
        raise ValueError('Bounded '+('ASCII ' if ascii_only else '')+name+' required')
    return value


def _code(value):
    try:
        valid = value is None or (type(value) in (int, float) and math.isfinite(value))
    except (ValueError, OverflowError):
        valid = False
    if not valid:
        raise ValueError('Explicit finite quality_code or None required')
    return value


def _offset(value):
    try:
        valid = type(value) in (int, float) and math.isfinite(value) and abs(value) <= 1_000_000
    except (ValueError, OverflowError):
        valid = False
    if not valid:
        raise ValueError('Finite bounded offset required')
    return float(value)


def store_offset_bp(value):
    """Apply original p.a(value,9): Math.round(value*1e9)/1e9.

    The multiplication is a double operation. Round its exact binary result
    to the nearest integer, with ties towards positive infinity. Integer-ratio
    arithmetic avoids rounding a subsequent floating-point addition of .5.
    Our bounded inputs keep the product within Java long's nonsaturating range.
    """
    product = _offset(value)*1_000_000_000.0
    numerator, denominator = product.as_integer_ratio()
    rounded = (2*numerator+denominator)//(2*denominator)
    return rounded/1_000_000_000.0


def _mean(values):
    # Python's sum() may compensate float errors; the observed Java p.c loop
    # adds each double in stored list order without compensation.
    total = 0.0
    for value in values:
        total += value
    return total/len(values)


def _identity(row):
    return row['marker'], row['key_kind'], row['bin_label'], row['ladder_id'].lower()


def _state(state):
    if state is None:
        return {'schema_version': 1, 'contributions': [], 'events': []}
    if (not isinstance(state, Mapping) or set(state) != {'schema_version', 'contributions', 'events'}
            or type(state['schema_version']) is not int or state['schema_version'] != 1
            or not isinstance(state['contributions'], list) or not isinstance(state['events'], list)):
        raise ValueError('Explicit versioned run offset state required')
    result = fastcopy(dict(state))
    seen = set()
    for row in result['contributions']:
        if not isinstance(row, Mapping) or set(row) != {
                'marker', 'key_kind', 'bin_label', 'ladder_id', 'offset_bp', 'quality_code'}:
            raise ValueError('Structured run offset contribution required')
        _name(row['marker'], 'marker', 128)
        _name(row['ladder_id'], 'ladder_id', ascii_only=True)
        if row['key_kind'] not in _KINDS:
            raise ValueError('Explicit bin/virtual_min/virtual_max key kind required')
        if row['key_kind'] == 'bin':
            _name(row['bin_label'], 'bin label', 128)
        elif row['bin_label'] is not None:
            raise ValueError('Virtual endpoints must have bin_label=None')
        _offset(row['offset_bp'])
        _code(row['quality_code'])
        identity = _identity(row)
        if identity in seen:
            raise ValueError('Duplicate per-key case-insensitive ladder contribution')
        seen.add(identity)
    return result


def remove_run_ladder(state, ladder_id):
    """Copy state and withdraw a filename from every key, ignoring ASCII case."""
    ladder_id = _name(ladder_id, 'ladder_id', ascii_only=True)
    result = _state(state)
    removed = [r for r in result['contributions'] if r['ladder_id'].lower() == ladder_id.lower()]
    result['contributions'] = [r for r in result['contributions']
                               if r['ladder_id'].lower() != ladder_id.lower()]
    result['events'].append({'action': 'remove_ladder', 'ladder_id': ladder_id,
                             'removed_contributions': len(removed),
                             'matched_ladder_ids': list(dict.fromkeys(r['ladder_id'] for r in removed)),
                             'identity_comparison': 'ASCII_equalsIgnoreCase'})
    return result


def validate_run_offset_state(state):
    """Return an own validated structured state; None denotes an empty run."""
    return _state(state)


def ladder_window_offset(state, marker, local_offset_bp=None):
    """Mean admitted previous-marker virtual-max values plus current local value.

    Corresponds to y.a(String,String,String,Sample)D, using the previous marker
    and VirtualBinMarkerSizeRangeMax. Local freshly matched offsets have no AL
    admission gate at this stage. Empty inputs return zero. The caller withdraws
    the current filename before supplying state for fresh reanalysis.
    """
    marker = _name(marker, 'marker', 128)
    state = _state(state)
    contributors = [fastcopy(r) for r in state['contributions']
                    if r['marker'] == marker and r['key_kind'] == 'virtual_max'
                    and r['quality_code'] in ALLOWED_CODES]
    values = [r['offset_bp'] for r in contributors]
    if local_offset_bp is not None:
        local_offset_bp = store_offset_bp(local_offset_bp)
        values.append(local_offset_bp)
    return {'marker': marker, 'offset_bp': _mean(values) if values else 0.0,
            'run_contributors': contributors, 'local_offset_bp': local_offset_bp,
            'value_count': len(values), 'empty_value': 0.0,
            'local_quality_gate_applied': False}


def upsert_run_offsets(state, ladder_id, quality_code, result):
    """Copy state and merge supplied accepted marker keys; absent keys survive.

    Rejected result/marker records contribute no keys. Accepted records may have
    partial bin or endpoint data: no whole-ladder completeness filter is added.
    None quality_code records remain visible but cannot enter numeric means.
    """
    ladder_id = _name(ladder_id, 'ladder_id', ascii_only=True)
    # quality_code may be one code for the whole ladder, or a {marker: code} mapping
    # (per-marker admission); a marker missing from the mapping is not admitted.
    if isinstance(quality_code, Mapping):
        marker_codes = {_name(m, 'marker', 128): _code(c) for m, c in quality_code.items()}
        event_code = dict(marker_codes)
    else:
        marker_codes = None
        quality_code = _code(quality_code)
        event_code = quality_code
    if (not isinstance(result, Mapping) or result.get('status') not in ('accepted', 'partial', 'rejected')
            or not isinstance(result.get('markers'), Mapping)):
        raise ValueError('Explicit ladder matching result and markers required')
    output = _state(state)
    incoming = []
    if result['status'] in ('accepted', 'partial'):
        for marker, record in result['markers'].items():
            _name(marker, 'marker', 128)
            if not isinstance(record, Mapping):
                raise ValueError('Explicit ladder marker result required')
            if record.get('status') != 'accepted':
                continue
            offsets = record.get('bin_offsets')
            if not isinstance(offsets, Mapping):
                raise ValueError('Explicit per-bin offset mapping required')
            row_code = marker_codes.get(marker) if marker_codes is not None else quality_code
            for label, value in offsets.items():
                _name(label, 'bin label', 128)
                incoming.append({'marker': marker, 'key_kind': 'bin', 'bin_label': label,
                                 'ladder_id': ladder_id, 'offset_bp': store_offset_bp(value),
                                 'quality_code': row_code})
            for field, kind in (('min_offset_bp', 'virtual_min'), ('max_offset_bp', 'virtual_max')):
                if record.get(field) is not None:
                    incoming.append({'marker': marker, 'key_kind': kind, 'bin_label': None,
                                     'ladder_id': ladder_id, 'offset_bp': store_offset_bp(record[field]),
                                     'quality_code': row_code})
    index = {_identity(row): i for i, row in enumerate(output['contributions'])}
    replaced = []
    for row in incoming:
        identity = _identity(row)
        if identity in index:
            position = index[identity]
            replaced.append(output['contributions'][position]['ladder_id'])
            output['contributions'][position] = row
        else:
            index[identity] = len(output['contributions'])
            output['contributions'].append(row)
    output['events'].append({'action': 'upsert_keys', 'ladder_id': ladder_id,
                             'quality_code': event_code, 'matching_status': result['status'],
                             'supplied_keys': len(incoming), 'replaced_keys': len(replaced),
                             'replaced_ladder_ids': list(dict.fromkeys(replaced)),
                             'absent_keys_preserved': True,
                             'identity_comparison': 'ASCII_equalsIgnoreCase'})
    return output


def aggregate_run_offsets(entries, kit):
    """Aggregate 1..128 explicit ladder analyses in supplied execution order.

    Each entry has ladder_id, quality_code (number or None), and raw matching
    result. A repeated filename withdraws ALL its previous keys first, including
    when fresh analysis is rejected or empty. The standalone upsert helper has
    the original narrower merge behavior. Means are independent for each bin
    and virtual endpoint; admitted codes are exactly 0, -2, and .5.
    """
    definition = load_kit_definition(kit)
    if not isinstance(entries, (list, tuple)) or not 1 <= len(entries) <= 128:
        raise ValueError('Explicit list of 1..128 ladder entries required')
    known = {m['name']: {b['label'] for b in m['bins']} for m in definition['markers']}
    state = _state(None)
    latest = {}
    history = []
    for entry in entries:
        if not isinstance(entry, Mapping) or set(entry) != {'ladder_id', 'quality_code', 'result'}:
            raise ValueError('ladder_id/quality_code/result required')
        identifier = _name(entry['ladder_id'], 'ladder_id', ascii_only=True)
        folded = identifier.lower()
        previous = latest.get(folded)
        withdrawn = 0
        if previous is not None:
            state = remove_run_ladder(state, identifier)
            withdrawn = state['events'][-1]['removed_contributions']
        state = upsert_run_offsets(state, identifier, entry['quality_code'], entry['result'])
        latest[folded] = identifier
        history.append({'ladder_id': identifier, 'quality_code': entry['quality_code'],
                        'matching_status': entry['result']['status'],
                        'replaced_ladder_id': previous, 'withdrawn_contributions': withdrawn,
                        'case_insensitive_replacement': previous is not None and previous != identifier})
    for row in state['contributions']:
        if row['marker'] not in known or (row['key_kind'] == 'bin'
                                         and row['bin_label'] not in known[row['marker']]):
            raise ValueError('Run contribution references unknown kit marker/bin')

    pools = {}
    for row in state['contributions']:
        pools.setdefault((row['marker'], row['key_kind'], row['bin_label']), []).append(row)

    def mean_for(marker, kind, label=None):
        rows = pools.get((marker, kind, label), [])
        admitted = [fastcopy(r) for r in rows if r['quality_code'] in ALLOWED_CODES]
        excluded = [{**fastcopy(r), 'reason': 'quality_code_missing' if r['quality_code'] is None
                     else 'quality_code_not_admitted'} for r in rows if r['quality_code'] not in ALLOWED_CODES]
        mean = _mean([r['offset_bp'] for r in admitted]) if admitted else None
        return {'offset_bp': mean, 'contributors': admitted, 'excluded': excluded}

    output = {'status': 'rejected', 'reasons': [], 'markers': {}, 'state': state,
              'diagnostics': {'method': 'per_key_unweighted_mean_after_fresh_filename_replacement',
                              'admitted_quality_codes': list(ALLOWED_CODES),
                              'quality_codes_computed_here': False,
                              'identity_comparison': 'ASCII_equalsIgnoreCase',
                              'source_contract': 'IDX16_ALLELE_CALLING AL-05/AL-07',
                              'entry_history': history, 'external_validation': False,
                              'original_runtime_validated': False,
                              'input_rounding': 'Math.round(double_value*1e9)/1e9; ties toward positive infinity',
                              'mean_arithmetic': 'sequential_double_sum_in_contribution_order',
                              'missing_key': 'None; no implicit zero or interpolation',
                              'marker_usable_policy': 'both_virtual_endpoints_and_any_direct_bin'}}
    for marker in definition['markers']:
        name = marker['name']
        bins = {b['label']: mean_for(name, 'bin', b['label']) for b in marker['bins']}
        low, high = mean_for(name, 'virtual_min'), mean_for(name, 'virtual_max')
        offsets = {label: row['offset_bp'] for label, row in bins.items() if row['offset_bp'] is not None}
        usable = bool(offsets) and low['offset_bp'] is not None and high['offset_bp'] is not None
        sources = list(dict.fromkeys(r['ladder_id'] for row in [*bins.values(), low, high]
                                     for r in row['contributors']))
        output['markers'][name] = {
            'status': 'accepted' if usable else 'rejected', 'bin_offsets': offsets,
            'min_offset_bp': low['offset_bp'], 'max_offset_bp': high['offset_bp'],
            'source_ladders': sources,
            'missing_bin_offsets': [label for label, row in bins.items() if row['offset_bp'] is None],
            'per_key': {'bins': bins, 'virtual_min': low, 'virtual_max': high},
        }
    accepted = sum(r['status'] == 'accepted' for r in output['markers'].values())
    output['status'] = 'accepted' if accepted == len(output['markers']) else 'partial' if accepted else 'rejected'
    if output['status'] != 'accepted':
        output['reasons'].append('Some markers lack admitted direct bins or virtual endpoints')
    return output
