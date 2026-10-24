"""Explicit research pipeline; calibration is withheld until matching and base QC pass.

The caller supplies expected fragments, extra-peak allowance and a quality
threshold. Defaults describe a chosen research protocol, not recovered user
settings. An independent Basic-standard autoheight is available only through
explicit height_mode selection. Normalization, broad-width adjustment, SQ
override, allele calling and UI interpretation remain outside this API.
"""
import math
from numbers import Real

from .preprocessing import prepare_trace
from .sg_peaks import detect_sg_peaks
from .standard_prefilters import filter_standard_peaks
from .ratio_sizing import match_ratio_standard
from .sizing_quality import evaluate_sizing_quality
from .autoheight import estimate_standard_autoheight
from .standard_wrapper import wrap_standard_peaks


def analyze_size_standard(values, expected_bp, *, num_extra_peaks, quality_threshold,
                          baseline_window=51, spike_window=5, baseline_smoothing_window=1,
                          smooth_passes=2, detection_threshold=100.0, sg_window=15,
                          min_width=2.0, tau_begin=0.0, tau_end=0.0, group_gap=25,
                          ratio_factor=0.8, min_accepted_score=0.75, ambiguity_margin=0.01,
                          allow_prefix=True, max_bp_per_scan=0.25, offscale_intervals=(),
                          height_mode='explicit', coordinate_mode='ap_fractional', sg_degree=3):
    """Analyze one explicit standard channel, returning anchors only after base SQ.

    Rejected paths may expose tentative matched_bp in selected_peaks for review.
    They never expose a calibration object or accepted anchors, even when the
    underlying ratio optimizer accepted the assignment before quality checks.
    Original post-sizing broad-width penalties and overrides remain excluded.
    height_mode='basic_standard_auto' calculates its independent threshold from
    the prepared signal; detection_threshold is used only in 'explicit' mode.
    coordinate_mode='standard_wrapper' applies integer coordinate conversion
    and subsequent offscale marking at offset0. Nonzero offsets are excluded
    from this local-signal pipeline. Default 'ap_fractional' retains the earlier
    experimental protocol for comparison.
    """
    result = {'status': 'rejected', 'reasons': [], 'stage': 'validation',
              'anchors': [], 'calibration': None, 'peaks': [], 'selected_peaks': [],
              'preparation': None, 'detector': None, 'filtering': None,
              'matching': None, 'base_quality': None, 'quality_threshold': None,
              'height_mode': height_mode, 'autoheight': None,
              'coordinate_mode': coordinate_mode, 'coordinate_conversion': None,
              'external_validation': False, 'original_runtime_validated': False,
              'not_applied': ['original autoheight', 'normalization', 'companion/jag/near-base features',
                              'wrapper integer coordinate conversion',
                              'ay broad-width multiplier', 'SQ override', 'original UI categories']}

    def reject(reason):
        result['reasons'].append(reason)
        return result

    try:
        quality_valid = (isinstance(quality_threshold, Real) and not isinstance(quality_threshold, bool)
                         and math.isfinite(quality_threshold) and 0 < quality_threshold <= 1)
    except (ValueError, OverflowError):
        quality_valid = False
    if not quality_valid:
        return reject('An explicit finite quality_threshold in (0, 1] is required')
    result['quality_threshold'] = float(quality_threshold)
    if height_mode not in ('explicit', 'basic_standard_auto'):
        return reject('height_mode must be explicit or basic_standard_auto')
    if coordinate_mode not in ('ap_fractional', 'standard_wrapper'):
        return reject('coordinate_mode must be ap_fractional or standard_wrapper')
    result['stage'] = 'preparation'
    try:
        prepared = prepare_trace(values, window=baseline_window, spike_window=spike_window,
                                 smoothing_window=baseline_smoothing_window, smooth_passes=smooth_passes)
    except ValueError as exc:
        return reject(str(exc))
    result['preparation'] = prepared['parameters']
    signal = prepared['signal']
    result['stage'] = 'detection'
    if height_mode == 'basic_standard_auto':
        result['stage'] = 'autoheight'
        try:
            autoheight = estimate_standard_autoheight(signal)
        except ValueError as exc:
            return reject(str(exc))
        detection_threshold = autoheight['threshold']
        result['autoheight'] = {key: value for key, value in autoheight.items() if key != 'control_signal'}
        result['not_applied'].remove('original autoheight')
        result['stage'] = 'detection'
    try:
        detected = detect_sg_peaks(signal, threshold=detection_threshold, window=sg_window, degree=sg_degree,
                                   min_width=min_width, tau_begin=tau_begin, tau_end=tau_end,
                                   offscale_intervals=offscale_intervals if coordinate_mode == 'ap_fractional' else ())
    except ValueError as exc:
        return reject(str(exc))
    result['peaks'], result['detector'] = detected['peaks'], detected['diagnostics']
    if coordinate_mode == 'standard_wrapper':
        result['stage'] = 'coordinate_conversion'
        try:
            projected = wrap_standard_peaks(detected['peaks'], signal_length=len(signal),
                                            offscale_intervals=offscale_intervals)
        except ValueError as exc:
            return reject(str(exc))
        result['peaks'] = projected['peaks']
        result['coordinate_conversion'] = projected['diagnostics']
        result['not_applied'].remove('wrapper integer coordinate conversion')
    result['stage'] = 'prefilters'
    expected_count = len(expected_bp) if isinstance(expected_bp, (list, tuple)) else None
    filtered = filter_standard_peaks(result['peaks'], expected_count, signal, group_gap=group_gap)
    result['filtering'] = filtered
    if filtered['status'] != 'passed':
        result['reasons'].extend(filtered['reasons'])
        return result
    result['stage'] = 'matching'
    remaining = filtered['peaks']
    match = match_ratio_standard(remaining, expected_bp, num_extra_peaks=num_extra_peaks,
                                ratio_factor=ratio_factor, min_accepted_score=min_accepted_score,
                                ambiguity_margin=ambiguity_margin, allow_prefix=allow_prefix,
                                max_bp_per_scan=max_bp_per_scan)
    result['matching'] = {key: value for key, value in match.items() if key not in ('anchors', 'calibration')}
    result['reasons'].extend(match['reasons'])
    if not match['selection']:
        return result
    assignments = {peak['candidate_index']: peak['expected_bp'] for peak in match['matched']}
    selected = [dict(remaining[i], matched_bp=assignments.get(i)) for i in match['selection']['candidate_indices']]
    result['selected_peaks'] = selected
    quality = evaluate_sizing_quality(selected, expected_bp)
    result['base_quality'] = quality
    if quality['status'] != 'evaluated':
        result['stage'] = 'base_quality'
        result['reasons'].extend(quality['reasons'])
        return result
    if quality['score'] < quality_threshold:
        result['stage'] = 'base_quality'
        return reject('Base sizing quality below explicit reporting threshold')
    if match['status'] != 'accepted':
        return result
    result.update(status='accepted', stage='complete', anchors=match['anchors'], calibration=match['calibration'])
    return result
