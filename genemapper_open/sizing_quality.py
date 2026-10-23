"""Independent sizing-quality mathematics from an inspected static contract.

The supplied peak features and assignments are inputs, not detected here. The
score combines explicit morphology penalties with a chi-square survival term.
It is not a probability of a correct standard or a runtime-equivalent ID-X SQ.
This module uses its own bounded input validation and stdlib numerical methods.
It deliberately requires exact expected-size coverage before calculating a
score; the inspected original method only checks the matched-count gate.
"""
from collections.abc import Mapping
import math
from numbers import Real
from statistics import median

MAX_PEAKS = 4096
MAX_EXPECTED = 32
_EPSILON = 2e-15
_TINY = 1e-300


def _regularized_gamma_q(shape, value):
    """Upper regularized incomplete gamma; series or modified Lentz fraction.

    The lower-tail series is used only where its complement is well resolved;
    the upper-tail fraction avoids subtracting a rounded CDF from one in tails.
    """
    if value == 0:
        return 1.0
    if math.isinf(value):
        return 0.0
    log_factor = shape*math.log(value)-value-math.lgamma(shape)
    if value < shape+1:
        term = total = 1.0/shape
        for step in range(1, 10001):
            term *= value/(shape+step)
            total += term
            if abs(term) <= abs(total)*_EPSILON:
                log_lower = min(0.0, log_factor+math.log(total))
                return min(1.0, max(0.0, -math.expm1(log_lower)))
        raise ArithmeticError("Incomplete-gamma series did not converge")
    denominator = value+1-shape
    inverse = 1.0/denominator
    numerator = 1.0/_TINY
    fraction = inverse
    for step in range(1, 10001):
        coefficient = -step*(step-shape)
        denominator += 2.0
        divisor = denominator+coefficient*inverse
        if abs(divisor) < _TINY:
            divisor = math.copysign(_TINY, divisor)
        numerator = denominator+coefficient/numerator
        if abs(numerator) < _TINY:
            numerator = math.copysign(_TINY, numerator)
        inverse = 1.0/divisor
        change = inverse*numerator
        fraction *= change
        if abs(change-1.0) <= _EPSILON:
            if fraction <= 0:
                raise ArithmeticError("Incomplete-gamma fraction became nonpositive")
            return min(1.0, max(0.0, math.exp(log_factor+math.log(fraction))))
    raise ArithmeticError("Incomplete-gamma fraction did not converge")


def chi_square_survival(statistic, degrees_of_freedom):
    """Return P(ChiSquared(df)>=statistic), for integer df1..1024 and x>=0.

    Infinite statistic returns zero. Invalid input raises ValueError. No SciPy,
    vendor numeric code, normal approximation or runtime reference is used.
    """
    if (not isinstance(statistic, Real) or isinstance(statistic, bool)
            or type(degrees_of_freedom) is not int or not 1 <= degrees_of_freedom <= 1024):
        raise ValueError("Finite nonnegative statistic or infinity, and integer df1..1024 required")
    try:
        value = float(statistic)
    except (ValueError, OverflowError):
        raise ValueError("Statistic cannot be represented") from None
    if math.isnan(value) or value < 0:
        raise ValueError("Statistic must be nonnegative")
    return _regularized_gamma_q(degrees_of_freedom/2.0, value/2.0)


def _bounded(value, minimum, maximum, *, positive=False):
    if not isinstance(value, Real) or isinstance(value, bool):
        return None
    try:
        number = float(value)
    except (ValueError, OverflowError):
        return None
    if not math.isfinite(number) or not minimum <= number <= maximum or (positive and number <= 0):
        return None
    return number


def evaluate_sizing_quality(peaks, expected_bp):
    """Return JSON diagnostics from sorted peak-feature mappings and assignments.

    Each peak has scan,height,begin_height,end_height and optional matched_bp
    (None or absent means unmatched). All scans must increase with gap>=1e-6.
    Heights are finite positive <=1e12; boundary heights may be signed <=1e12.
    Explicit expected sizes contain3..32 increasing positive values <=1e6.

    A valid but incomplete/mismatched assignment has status='evaluated', score0
    and a reason. Malformed input has status='rejected', score=None. The function
    does not apply an arbitrary universal acceptance threshold. Unmatched
    peaks outside the matched interval do not affect penalties. Callers choose
    which candidate set to supply; this function does not perform selection.

    Own numerical choices include math.fsum, math.lgamma and direct upper-tail
    evaluation, rather than the inspected vendor CDF/1-CDF implementation.
    Four-place score rounding is nonnegative half-up, matching the documented
    mathematical rounding rule without claiming JVM bit equivalence.
    """
    result = {'status': 'rejected', 'score': None, 'raw_score': None,
              'penalty_factor': 1.0, 'reasons': [], 'events': [], 'complete': False,
              'original_count_gate_passed': False, 'matched_count': 0,
              'expected_count': 0, 'chi_square_statistic': None,
              'degrees_of_freedom': None, 'chi_square_survival': None,
              'global_bp_per_scan': None, 'original_runtime_validated': False,
              'external_validation': False, 'score_is_original_sq': False,
              'score_rounding': 'nonnegative_half_up_4_decimal_places',
              'method': 'independent_morphology_penalty_times_chi_square_survival'}

    def invalid(reason):
        result['reasons'].append(reason)
        return result

    def zero(reason):
        result.update(status='evaluated', score=0.0, raw_score=0.0)
        result['reasons'].append(reason)
        return result

    if not isinstance(expected_bp, (list, tuple)) or not 3 <= len(expected_bp) <= MAX_EXPECTED:
        return invalid('expected_bp must contain3..32 explicit increasing sizes')
    expected = [_bounded(value, 0, 1_000_000, positive=True) for value in expected_bp]
    if any(value is None for value in expected) or any(b-a < 1e-6 for a, b in zip(expected, expected[1:])):
        return invalid('expected_bp requires finite positive values and gaps>=0.000001bp')
    result['expected_count'] = len(expected)
    if not isinstance(peaks, (list, tuple)) or len(peaks) > MAX_PEAKS:
        return invalid('peaks must be a list or tuple of at most4096 mappings')
    normalized = []
    for index, peak in enumerate(peaks):
        if not isinstance(peak, Mapping):
            return invalid(f'peak{index} must be a feature mapping')
        scan = _bounded(peak.get('scan'), 0, 100_000_000)
        height = _bounded(peak.get('height'), 0, 1_000_000_000_000, positive=True)
        beginning = _bounded(peak.get('begin_height'), -1_000_000_000_000, 1_000_000_000_000)
        end = _bounded(peak.get('end_height'), -1_000_000_000_000, 1_000_000_000_000)
        if any(value is None for value in (scan, height, beginning, end)):
            return invalid(f'peak{index} has invalid scan/height/boundary features')
        assignment = peak.get('matched_bp')
        if assignment is not None:
            assignment = _bounded(assignment, 0, 1_000_000, positive=True)
            if assignment is None:
                return invalid(f'peak{index} has invalid matched_bp')
        normalized.append({'index': index, 'scan': scan, 'height': height,
                           'begin_height': beginning, 'end_height': end, 'matched_bp': assignment})
    if any(b['scan']-a['scan'] < 1e-6 for a, b in zip(normalized, normalized[1:])):
        return invalid('input peaks must already have strictly increasing scans with gaps>=0.000001')
    matched = [peak for peak in normalized if peak['matched_bp'] is not None]
    result['matched_count'] = len(matched)
    result['original_count_gate_passed'] = len(matched) >= len(expected)
    if len(matched) < len(expected):
        return zero('incomplete_standard')
    if [peak['matched_bp'] for peak in matched] != expected:
        return zero('matched_sizes_do_not_equal_expected_standard')
    result['complete'] = True

    penalty = 1.0

    def apply(kind, peak, factor):
        nonlocal penalty
        penalty *= factor
        result['events'].append({'kind': kind, 'peak_index': peak['index'], 'factor': factor})
        result['penalty_factor'] = penalty

    for field in ('begin_height', 'end_height'):
        average = math.fsum(peak[field] for peak in matched)/len(matched)
        for peak in matched:
            if peak[field] > 10*average and peak[field]/peak['height'] > 0.25:
                apply(field+'_outlier', peak, 0.5)
    center_height = median(peak['height'] for peak in matched)
    for peak in matched:
        if peak['height'] > 8*center_height:
            apply('height_above_8_medians', peak, 0.5)
    for left, right in zip(matched, matched[1:]):
        if right['height']/left['height'] > 10:
            return zero('directed_neighbor_height_ratio_above10')
    for left, right in zip(matched, matched[1:]):
        average = (left['height']+right['height'])/2
        for peak in normalized[left['index']+1:right['index']]:
            if peak['matched_bp'] is not None:
                continue
            if peak['height'] > 5*average:
                return zero('unmatched_between_anchors_above5_mean')
            if peak['height'] > 0.7*average:
                apply('unmatched_between_anchors_above0_7_mean', peak, 0.9)
    rate = (matched[-1]['matched_bp']-matched[0]['matched_bp'])/(matched[-1]['scan']-matched[0]['scan'])
    result['global_bp_per_scan'] = rate
    if rate > 0.25:
        return zero('global_migration_rate_above0_25')
    statistic = math.fsum(((rate*(right['scan']-left['scan'])-(right['matched_bp']-left['matched_bp']))**2)
                          /(right['matched_bp']-left['matched_bp']) for left, right in zip(matched, matched[1:]))
    degrees = len(matched)-2
    try:
        survival = chi_square_survival(statistic, degrees)
    except (ValueError, ArithmeticError, OverflowError):
        return invalid('chi_square_numerical_evaluation_failed')
    raw = min(1.0, max(0.0, penalty*survival))
    result.update(status='evaluated', score=math.floor(raw*10000+0.5)/10000,
                  raw_score=raw, chi_square_statistic=statistic,
                  degrees_of_freedom=degrees, chi_square_survival=survival)
    return result
