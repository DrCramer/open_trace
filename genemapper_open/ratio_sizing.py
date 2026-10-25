"""Independent optimizer for an explicit height/spacing ratio objective.

The objective is documented from static ID-X research, but this is a new,
bounded implementation: two distinct best assignments, explicit selection,
complete-prefix diagnostics, ambiguity rejection and a separate migration-rate
gate. This score is not original SQ or a probability. No vendor runtime, AP
morphology, original early/group filters or kit constants are included.
"""
from collections.abc import Mapping
import math
from numbers import Real

from . import _core
from .analysis import Calibration

MAX_INPUT_PEAKS = 4096
MAX_CANDIDATES = 64
MAX_EXPECTED = 32
SCORE_TOLERANCE = 1e-12


def _number(value):
    if not isinstance(value, Real) or isinstance(value, bool):
        return False
    try:
        return math.isfinite(value)
    except (ValueError, OverflowError):
        return False


def _best_paths(scans, heights, bp, weight):
    """Rust fast path when available and within its size limits; the Python reference otherwise."""
    core = _core.CORE
    if core is not None and hasattr(core, 'best_paths') and len(scans) <= 255 and 3 <= len(bp) <= 32 \
            and len(scans) >= len(bp) and len(scans) == len(heights):
        paths, transitions = core.best_paths([float(v) for v in scans], [float(v) for v in heights],
                                             [float(v) for v in bp], float(weight))
        return [(score, tuple(path)) for score, path in paths], transitions
    return _best_paths_py(scans, heights, bp, weight)


def _best_paths_py(scans, heights, bp, weight):
    """Two best paths per last-two state; future cost depends only on that pair."""
    count, length = len(scans), len(bp)
    scale = max(heights)
    # Normalize before multiplying: finite subnormal input heights must not
    # underflow merely because the RFU unit was rescaled.
    height_term = [(1-weight)*(height/scale) for height in heights]
    states = {(a, b): [(height_term[a]+height_term[b], (a, b))]
              for b in range(1, count-length+2) for a in range(b)}
    transitions = 0
    for marker in range(2, length):
        target_ratio = (bp[marker]-bp[marker-1])/(bp[marker-1]-bp[marker-2])
        next_states = {}
        for (a, b), paths in states.items():
            previous_gap = scans[b]-scans[a]
            for c in range(b+1, count-length+marker+1):
                transitions += 1
                scan_ratio = (scans[c]-scans[b])/previous_gap
                agreement = min(scan_ratio/target_ratio, target_ratio/scan_ratio)
                options = next_states.setdefault((b, c), [])
                for score, path in paths:
                    options.append(((score+weight*agreement)+height_term[c], path+(c,)))
                options.sort(key=lambda item: (-item[0], item[1]))
                del options[2:]
        states = next_states
    choices = sorted((item for paths in states.values() for item in paths),
                     key=lambda item: (-item[0], item[1]))[:2]
    denominator = length-2*weight
    return [(score/denominator, path) for score, path in choices], transitions


def match_ratio_standard(peaks, expected_bp, *, num_extra_peaks, ratio_factor=0.8,
                         min_accepted_score=0.75, ambiguity_margin=0.01,
                         allow_prefix=False, max_bp_per_scan=0.25):
    """Return a bounded diagnostic result; calibration requires a complete match.

    All candidate heights must be positive. Input scan order is normalized;
    duplicate scans are rejected. Explicitly retain at most N+num_extra_peaks
    tallest candidates, favoring later scan-order indices at tied cutoff height,
    and clip scoring heights to the Nth tallest input height. Input peaks are
    never mutated. Original morphology prefilters are not applied here.

    For an increasing assignment j of K markers, maximize
      ((1-r)*sum(H[j]/Hmax) + r*sum(q)) / (K-2*r),
    where q compares adjacent scan-gap and bp-gap ratios via min(x/y,y/x).
    If allow_prefix, try expected prefixes N,N-1,...,2 only while score is below
    threshold. A prefix never emits calibration; it is an incomplete diagnostic.
    At r=1 the two-marker objective is undefined and is not attempted.

    Unlike the original one-best search, two distinct assignments are retained
    to expose ambiguity. Equal-score representative paths use lexicographic
    candidate indices, which is an explicit API choice, not original tie order.
    The optional rate bound is checked after optimization, not used to silently
    replace the objective or choose a lower-ranked path. None disables this gate.
    Threshold and ambiguity comparisons use an absolute score tolerance 1e-12.
    """
    result = {'status': 'rejected', 'reasons': [], 'anchors': [], 'calibration': None,
              'matched': [], 'selection': {}, 'attempts': [], 'diagnostics': {
                  'method': 'height_spacing_ratio_second_order_dp', 'score_is_original_sq': False,
                  'external_validation': False, 'transitions_checked': 0,
                  'score_comparison_tolerance': SCORE_TOLERANCE}}

    def reject(reason):
        result['reasons'].append(reason)
        return result

    if not _number(ratio_factor) or not 0 <= ratio_factor <= 1:
        return reject('ratio_factor must be finite and in [0, 1]')
    if not _number(min_accepted_score) or not 0 < min_accepted_score <= 1:
        return reject('min_accepted_score must be finite and in (0, 1]')
    if not _number(ambiguity_margin) or not 0 <= ambiguity_margin <= 1:
        return reject('ambiguity_margin must be finite and in [0, 1]')
    if type(allow_prefix) is not bool:
        return reject('allow_prefix must be boolean')
    if max_bp_per_scan is not None and (not _number(max_bp_per_scan) or not 0 < max_bp_per_scan <= 100):
        return reject('max_bp_per_scan must be None or finite and in (0, 100]')
    if type(num_extra_peaks) is not int or not 0 <= num_extra_peaks <= 32:
        return reject('num_extra_peaks must be an integer in [0, 32]')
    if not isinstance(expected_bp, (list, tuple)) or not 3 <= len(expected_bp) <= MAX_EXPECTED:
        return reject('Expected 3..32 explicit fragment sizes')
    if any(not _number(x) or not 0 < x <= 1_000_000 for x in expected_bp):
        return reject('Expected fragment sizes must be finite and in (0, 1000000]')
    bp = [float(x) for x in expected_bp]
    if any(b-a < 1e-6 for a, b in zip(bp, bp[1:])):
        return reject('Expected fragment sizes must increase with gaps >= 0.000001 bp')
    if not isinstance(peaks, (list, tuple)) or not 1 <= len(peaks) <= MAX_INPUT_PEAKS:
        return reject('Candidates must be an explicit list or tuple of 1..4096 peaks')
    candidates = []
    for index, peak in enumerate(peaks):
        if (not isinstance(peak, Mapping) or not _number(peak.get('scan'))
                or not 0 <= peak['scan'] <= 100_000_000 or not _number(peak.get('height'))
                or not 0 < peak['height'] <= 1_000_000_000_000):
            return reject(f'Candidate {index}: bounded finite nonnegative scan and positive height required')
        candidates.append({'candidate_index': index, 'scan': float(peak['scan']), 'height': float(peak['height'])})
    candidates.sort(key=lambda p: p['scan'])
    if any(b['scan']-a['scan'] < 1e-6 for a, b in zip(candidates, candidates[1:])):
        return reject('Candidate scans must increase with gaps >= 0.000001 scans')
    expected_count = len(bp)
    if len(candidates) < expected_count:
        return reject('Fewer candidates than the full expected standard')
    limit = expected_count+num_extra_peaks
    if limit > MAX_CANDIDATES:
        return reject('Selected candidate limit exceeds 64')
    ranked = sorted(range(len(candidates)), key=lambda i: (candidates[i]['height'], i))
    cutoff = candidates[ranked[-expected_count]]['height']
    indices = sorted(ranked[-limit:])
    selected = [candidates[i] for i in indices]
    result['selection'] = {'input_count': len(candidates), 'selected_count': len(selected),
                           'expected_count': expected_count, 'extra_peaks': num_extra_peaks,
                           'height_clip': cutoff, 'policy': 'explicit tallest N+extra; later index wins cutoff ties',
                           'candidate_indices': [p['candidate_index'] for p in selected]}
    heights = [min(p['height'], cutoff) for p in selected]
    scans = [p['scan'] for p in selected]
    lower = 2 if allow_prefix and ratio_factor < 1 else 3 if allow_prefix else expected_count
    choices = []
    for length in range(expected_count, lower-1, -1):
        choices, transitions = _best_paths(scans, heights, bp[:length], float(ratio_factor))
        result['diagnostics']['transitions_checked'] += transitions
        best = choices[0][0]
        result['attempts'].append({'expected_prefix_count': length, 'best_score': best})
        if best+SCORE_TOLERANCE >= min_accepted_score or not allow_prefix:
            break
    best, path = choices[0]
    second = choices[1][0] if len(choices) > 1 else None
    gap = best-second if second is not None else None
    result['matched'] = [dict(selected[index], expected_bp=bp[k]) for k, index in enumerate(path)]
    rate = (bp[len(path)-1]-bp[0])/(scans[path[-1]]-scans[path[0]])
    result['diagnostics'].update({'best_score': best, 'second_best_score': second, 'ambiguity_gap': gap,
                                  'ratio_factor': float(ratio_factor), 'min_accepted_score': float(min_accepted_score),
                                  'ambiguity_margin': float(ambiguity_margin), 'complete': len(path) == expected_count,
                                  'global_bp_per_scan': rate, 'max_bp_per_scan': max_bp_per_scan})
    if best+SCORE_TOLERANCE < min_accepted_score:
        return reject('No attempted prefix reaches the score threshold')
    if len(path) != expected_count:
        return reject('Only an incomplete expected prefix reaches the score threshold; calibration withheld')
    if gap is not None and gap <= ambiguity_margin+SCORE_TOLERANCE:
        return reject('Ambiguous ratio assignments; calibration withheld')
    if max_bp_per_scan is not None and rate > max_bp_per_scan:
        return reject('Assignment exceeds the explicit migration-rate bound; calibration withheld')
    result['anchors'] = [[p['scan'], p['expected_bp']] for p in result['matched']]
    result['calibration'] = Calibration(result['anchors'])
    result['status'] = 'accepted'
    return result
