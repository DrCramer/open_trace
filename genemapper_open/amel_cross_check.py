"""Independent fresh-result AMELCC subset from retained static observations.

Explicit panel roles and sample weight only. No sex inference or clinical
interpretation. Call confidences are intentionally outside this processor.
"""
from collections.abc import Mapping
from .quality_control import _number, _bool


def evaluate_amel_cross_check(markers, *, sample_type, weight):
    """Return GQ updates; never mutate the input or infer absent marker roles.

    Names follow the original validator's case-sensitive entry gate, then the
    processor's ASCII case-insensitive selection. Missing required metrics
    produce unavailable rather than a partially applied exception path.
    """
    if sample_type not in ('Sample', 'Positive Control', 'Negative Control', 'Allelic Ladder'):
        raise ValueError('Explicit canonical sample type required')
    weight = _number(weight, 'AMELCC sample weight', 0, 1)
    if not isinstance(markers, (list, tuple)) or len(markers) > 128:
        raise ValueError('At most128 explicit marker observations required')
    prepared = []
    for marker in markers:
        if not isinstance(marker, Mapping) or set(marker) != {'name', 'is_y_marker', 'calls', 'gq'}:
            raise ValueError('Explicit name/is_y_marker/calls/gq required')
        name = marker['name']
        if not isinstance(name, str) or not 1 <= len(name) <= 128 or not name.isascii():
            raise ValueError('Bounded ASCII marker name required')
        _bool(marker['is_y_marker'], 'is_y_marker')
        if not isinstance(marker['calls'], (list, tuple)) or len(marker['calls']) > 4096:
            raise ValueError('At most4096 calls per marker required')
        for call in marker['calls']:
            if not isinstance(call, Mapping) or set(call) != {'label', 'off_ladder'}:
                raise ValueError('Explicit label/off_ladder required')
            label = call['label']
            if not isinstance(label, str) or not 1 <= len(label) <= 128 or not label.isascii():
                raise ValueError('Bounded ASCII label required')
            _bool(call['off_ladder'], 'off_ladder')
        if marker['gq'] is not None:
            _number(marker['gq'], 'marker GQ', 0, 1)
        prepared.append(marker)
    if len({m['name'] for m in prepared}) != len(prepared):
        raise ValueError('Unique marker names required')
    result = {'status': 'not_applicable', 'reason': None, 'weight': weight,
              'mismatch': None, 'updates': [], 'external_validation': False,
              'call_confidences_recomputed': False,
              'scope': 'fresh_results_without_overrides_or_partial_exception_mutations'}
    if sample_type in ('Negative Control', 'Allelic Ladder'):
        result['reason'] = 'sample_type_excluded'
        return result
    if not prepared:
        result.update(status='unavailable', reason='panel_markers_missing')
        return result
    validator_amel=next((m for m in prepared if m['name'] in ('AMEL', 'Amelogenin')),None)
    if validator_amel is None:
        result['reason'] = 'validator_exact_amel_name_missing'
        return result
    if validator_amel['gq'] is None:
        result.update(status='unavailable', reason='validator_amel_quality_missing')
        return result
    amel = next(m for m in prepared if m['name'].lower() in ('amel', 'amelogenin'))
    if amel['gq'] is None:
        result.update(status='unavailable', reason='amel_quality_missing')
        return result
    ys = [m for m in prepared if m['is_y_marker'] and m['name'].lower() not in ('amel', 'amelogenin')]
    if not ys:
        result['reason'] = 'no_y_markers'
        return result
    if any(m['gq'] is None for m in ys):
        result.update(status='unavailable', reason='y_marker_quality_missing')
        return result
    labels = [c['label'].lower() for c in amel['calls']]
    count = (1 + int('y' in labels)) if 'x' in labels else -1
    if count != len(labels):
        count = -1
    mismatch = count < 1
    reason = 'invalid_amel_combination' if mismatch else 'consistent_amel_y_calls'
    mismatch_marker = None
    for marker in ys:
        if mismatch:
            break
        called = sum(not c['off_ladder'] for c in marker['calls'])
        if (count == 1 and called > 0) or (count == 2 and called == 0):
            mismatch = True
            reason = 'amel_y_presence_mismatch'
            mismatch_marker = marker['name']
    result.update(status='evaluated_subset', reason=reason, mismatch=mismatch,
                  amel_marker=amel['name'], amel_call_count=count,
                  first_mismatch_marker=mismatch_marker)
    for marker in [*ys, amel]:
        before = float(marker['gq'])
        result['updates'].append({'marker': marker['name'], 'metric': int(mismatch),
                                  'gq_before': before,
                                  'gq_after': before * (1 - weight) if mismatch else before})
    return result
