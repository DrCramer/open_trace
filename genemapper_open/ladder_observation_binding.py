"""Consistency binding for our own ladder observations and their source inputs.

The digests detect stale or changed inputs; they do not authenticate a caller.
Only the declared downstream spike transition may change a bound observation.
Validation can replay that transition, never ladder matching or allele calling.
"""
from collections.abc import Mapping
from .fastcopy import fastcopy
import hashlib
import json
import re

from .allele_calling import _prepare_peaks
from .kit_definition import load_kit_definition


_KEY = 'observation_binding'
_FIELDS = {'schema_version', 'scope', 'source_sha256', 'kit_sha256',
           'matching_sha256', 'observations_sha256'}
_SCOPE = 'consistency_only_not_caller_authentication'


def _digest(value):
    try:
        encoded = json.dumps(value, sort_keys=True, separators=(',', ':'),
                             ensure_ascii=False, allow_nan=False).encode('utf-8')
    except (TypeError, ValueError, OverflowError, RecursionError) as error:
        raise ValueError('JSON-compatible finite ladder binding inputs required') from error
    return hashlib.sha256(encoded).hexdigest()


def _without_binding(observations):
    if not isinstance(observations, Mapping):
        raise ValueError('Ladder observations must be an object')
    result = fastcopy(dict(observations))
    result.pop(_KEY, None)
    markers = result.get('markers')
    if not isinstance(markers, list) or any(not isinstance(m, Mapping) for m in markers):
        raise ValueError('Explicit ladder observation marker list required')
    return result


def _immutable(observations):
    result = _without_binding(observations)
    result.pop('spike_adjustments', None)
    for marker in result['markers']:
        marker.pop('spike_state', None)
    return result


def _sources(peaks, initial_spikes=None):
    # Normalization retains original input-index identity, not sorted position.
    prepared = sorted(_prepare_peaks(peaks), key=lambda p: p['peak_index'])
    result = []
    for peak in prepared:
        index = peak['peak_index']
        flags = set(peak['flags'])
        if initial_spikes is not None:
            flags.discard('spike')
            if initial_spikes[index]:
                flags.add('spike')
        source = peaks[index]
        result.append({**peak, 'flags': sorted(flags),
                       'id': {'present': 'id' in source, 'value': source.get('id')},
                       'offscaled': {'present': 'offscaled' in source,
                                     'value': source.get('offscaled')}})
    return result


def _check_digest(actual, expected, name):
    if _digest(actual) != expected:
        raise ValueError('Bound ladder '+name+' changed or is stale')


def bind(peaks, kit, matching, observations):
    """Copy fresh observations and bind their immutable structure to inputs.

    Call immediately after producing observations, before spike adjustment.
    This records consistency, not independent correctness or authentication.
    """
    sources = _sources(peaks)
    result = _without_binding(observations)
    if ('spike_adjustments' in result
            or any('spike_state' in m for m in result['markers'])
            or any('spike_adjustment' in p for p in peaks)):
        raise ValueError('Bind fresh ladder observations before spike adjustment')
    if not isinstance(matching, Mapping):
        raise ValueError('Explicit ladder matching object required')
    result[_KEY] = {
        'schema_version': 1, 'scope': _SCOPE,
        'source_sha256': _digest(sources),
        'kit_sha256': _digest(load_kit_definition(kit)),
        'matching_sha256': _digest(matching),
        'observations_sha256': _digest(result),
    }
    return result


def _validate_adjusted(peaks, observations, binding):
    summary = observations['spike_adjustments']
    if not isinstance(summary, Mapping):
        raise ValueError('Explicit ladder spike adjustment summary required')
    states = summary.get('peak_states')
    if not isinstance(states, list) or len(states) != len(peaks):
        raise ValueError('Complete spike adjustment source states required')
    initial = {}
    current = {}
    by_index = {}
    for state in states:
        if not isinstance(state, Mapping):
            raise ValueError('Explicit spike adjustment peak state required')
        index = state.get('peak_index')
        if type(index) is not int or not 0 <= index < len(peaks) or index in initial:
            raise ValueError('Unique exact spike adjustment input indices required')
        if type(state.get('initial_spike')) is not bool or type(state.get('current_spike')) is not bool:
            raise ValueError('Explicit initial/current spike booleans required')
        initial[index] = state['initial_spike']
        current[index] = state['current_spike']
        by_index[index] = state
    _check_digest(_sources(peaks, initial), binding['source_sha256'], 'source inputs')

    source_states = {p['peak_index']: 'spike' in p['flags'] for p in _prepare_peaks(peaks)}
    if source_states != initial and source_states != current:
        raise ValueError('Source spike flags do not match a complete bound phase')
    has_metadata = any('spike_adjustment' in p for p in peaks)
    if has_metadata:
        if source_states != current or any('spike_adjustment' not in p for p in peaks):
            raise ValueError('Applied source spike metadata requires the complete current phase')
        for index, peak in enumerate(peaks):
            if _digest(peak['spike_adjustment']) != _digest(by_index[index]):
                raise ValueError('Source spike adjustment metadata differs from summary')

    # Restore only initial spike flags. All other source observations must have
    # passed their original digest above, and all call/window data is immutable.
    normalized = {p['peak_index']: p for p in _prepare_peaks(peaks)}
    restored = fastcopy(peaks)
    for index, peak in enumerate(restored):
        peak['flags'] = [f for f in normalized[index]['flags'] if f != 'spike']
        peak['spike'] = initial[index]
        if initial[index]:
            peak['flags'].append('spike')
        peak.pop('spike_adjustment', None)
    from .spike_adjustment import adjust_ladder_spikes
    try:
        replayed = adjust_ladder_spikes(restored, _immutable(observations), summary.get('policy'))
    except (KeyError, TypeError, ValueError, IndexError, OverflowError) as error:
        raise ValueError('Invalid bound ladder spike adjustment phase') from error
    if _digest(replayed) != _digest(_without_binding(observations)):
        raise ValueError('Ladder spike adjustment phase differs from the declared transition')


def validate(peaks, kit, matching, observations):
    """Return an own validated copy, or raise ValueError for a stale binding.

    Accept initial source flags, or the complete current flags produced by
    apply_peak_states. Adjusted observations are checked by replaying only
    adjust_ladder_spikes against restored initial flags and immutable calls.
    """
    sources = _sources(peaks)
    result = _without_binding(observations)
    binding = observations.get(_KEY)
    if (not isinstance(binding, Mapping) or set(binding) != _FIELDS
            or type(binding.get('schema_version')) is not int or binding['schema_version'] != 1
            or binding.get('scope') != _SCOPE):
        raise ValueError('Versioned ladder observation consistency binding required')
    for key in _FIELDS - {'schema_version', 'scope'}:
        if not isinstance(binding[key], str) or not re.fullmatch('[0-9a-f]{64}', binding[key]):
            raise ValueError('Valid ladder observation consistency digests required')
    _check_digest(load_kit_definition(kit), binding['kit_sha256'], 'kit')
    _check_digest(matching, binding['matching_sha256'], 'matching')
    _check_digest(_immutable(result), binding['observations_sha256'], 'observation structure')
    if 'spike_adjustments' in result:
        _validate_adjusted(peaks, result, binding)
    else:
        if (any('spike_state' in m for m in result['markers'])
                or any('spike_adjustment' in p for p in peaks)):
            raise ValueError('Spike phase metadata requires a checked adjustment summary')
        _check_digest(sources, binding['source_sha256'], 'source inputs')
    result[_KEY] = fastcopy(dict(binding))
    return result
