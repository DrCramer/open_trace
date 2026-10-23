"""Independent experimental alignment of a user-specified size standard.

The standard consists solely of explicit expected fragment sizes. This module
has no kit definitions, manufacturer code, or trained parameters. It requires
every expected marker, permits additional observed peaks, and compares the two
best distinct ordered assignments before constructing any calibration.

Matching minimizes the RMS of interior leave-one-out *linear interpolation*
errors. A marker's own scan/bp pair is omitted when its error is calculated.
These are internal consistency diagnostics, not external validation or proof
that the correct biological markers were found. Smooth migration curvature is
allowed up to an explicit bp residual limit. No extrapolation is performed by
the resulting calibration.
"""
from collections.abc import Mapping
import math
from numbers import Real
import re

from .analysis import Calibration

MAX_CANDIDATES = 64
MAX_EXPECTED = 32
MIN_EXPECTED = 4
_MAX_SCAN = 100_000_000
_MAX_BP = 1_000_000


def _finite_number(value):
    if not isinstance(value, Real) or isinstance(value, bool):
        return False
    try:
        return math.isfinite(value)
    except (ValueError, OverflowError):
        return False


def parse_expected_bp(text):
    """Parse 4..32 increasing user fragment sizes; invalid input raises ValueError.

    Commas, semicolons, spaces and newlines are separators. The text-length
    check precedes splitting, and bounded splitting prevents huge token lists.
    There are no default fragments or kit-specific definitions.
    """
    if not isinstance(text, str) or len(text) > 4096:
        raise ValueError("Expected fragment text must be a string of at most 4096 characters")
    parts = re.split(r"[,;\s]+", text.strip(), maxsplit=MAX_EXPECTED)
    if not MIN_EXPECTED <= len(parts) <= MAX_EXPECTED:
        raise ValueError(f"Enter {MIN_EXPECTED}..{MAX_EXPECTED} expected fragment sizes")
    try:
        sizes = [float(part) for part in parts]
    except ValueError as exc:
        raise ValueError("Expected fragment sizes must be numbers separated by commas or whitespace") from exc
    if any(not math.isfinite(bp) or not 0 < bp <= _MAX_BP for bp in sizes):
        raise ValueError("Expected fragment sizes must be finite, positive and <= 1000000 bp")
    if any(b <= a for a, b in zip(sizes, sizes[1:])):
        raise ValueError("Expected fragment sizes must increase strictly; duplicates are invalid")
    return sizes


def _initial_result():
    return {"status": "rejected", "reasons": [], "anchors": [],
            "candidate_count": 0, "expected_count": 0,
            "matched": [], "unmatched": [], "diagnostics": {
                "method": "ordered_second_order_dp",
                "quality_metric": "interior_leave_one_out_linear_interpolation_bp",
                "external_validation": False,
                "max_candidates": MAX_CANDIDATES, "max_expected": MAX_EXPECTED,
                "transitions_checked": 0}, "calibration": None}


def _error_bp(left_scan, middle_scan, right_scan, left_bp, middle_bp, right_bp):
    """Predict middle bp from the two neighbors, excluding the middle anchor."""
    return left_bp + (middle_scan - left_scan) / (right_scan - left_scan) * (right_bp - left_bp) - middle_bp


def match_size_standard(peaks, expected_bp, *, max_residual_bp=5.0,
                        ambiguity_margin_bp=0.5):
    """Return a structured accepted/rejected result, including all diagnostics.

    ``peaks`` is a list/tuple of mappings with finite nonnegative ``scan`` and
    optionally finite ``height``. Peak height is recorded but never used to
    select candidates. Scans must be distinct; input order is immaterial.
    ``expected_bp`` must contain 4..32 finite positive strictly increasing
    sizes. Every size must match a distinct candidate. At most 64 candidate
    peaks are accepted; callers should explicitly select a scan interval or
    threshold rather than silently trimming the tallest peaks.

    Every interior leave-one-out residual must be <= ``max_residual_bp``.
    Among feasible complete assignments the minimum RMS residual wins. The
    result is rejected when another assignment has RMS within the absolute
    ``ambiguity_margin_bp`` of the best. At least two best paths are retained
    for each dynamic-program state, including alternatives with the same last
    two peaks. This prevents tie-breaking from hiding ambiguous alignments.

    ``anchors`` and ``calibration`` are populated only on acceptance. On an
    ambiguous rejection, ``matched`` is the best *tentative* assignment and
    ``diagnostics.alternative_match`` identifies its competitor. Endpoint
    residuals are None: they cannot be held out without extrapolating.

    Settings/input problems return a rejected result with an explicit reason;
    they do not trigger unbounded iteration. Worst-case state transitions are
    bounded by 2 * MAX_EXPECTED * choose(MAX_CANDIDATES, 3).
    """
    result = _initial_result()
    diagnostics = result["diagnostics"]

    def reject(reason):
        result["reasons"].append(reason)
        return result

    if not _finite_number(max_residual_bp) or not 0 < max_residual_bp <= 100:
        return reject("max_residual_bp must be finite and in (0, 100]")
    if not _finite_number(ambiguity_margin_bp) or not 0 <= ambiguity_margin_bp <= 100:
        return reject("ambiguity_margin_bp must be finite and in [0, 100]")
    diagnostics.update({"residual_limit_bp": float(max_residual_bp),
                        "ambiguity_margin_bp": float(ambiguity_margin_bp)})
    if not isinstance(expected_bp, (list, tuple)):
        return reject("expected_bp must be an explicit bounded list or tuple")
    result["expected_count"] = len(expected_bp)
    if not MIN_EXPECTED <= len(expected_bp) <= MAX_EXPECTED:
        return reject(f"Expected {MIN_EXPECTED}..{MAX_EXPECTED} fragment sizes")
    if any(not _finite_number(bp) or not 0 < bp <= _MAX_BP for bp in expected_bp):
        return reject("Expected fragment sizes must be finite, positive and <= 1000000 bp")
    bp = [float(value) for value in expected_bp]
    if any(b <= a for a, b in zip(bp, bp[1:])):
        return reject("Expected fragment sizes must increase strictly; duplicates are invalid")
    if not isinstance(peaks, (list, tuple)):
        return reject("Candidates must be an explicit bounded list or tuple")
    result["candidate_count"] = len(peaks)
    if len(peaks) > MAX_CANDIDATES:
        return reject(f"More than {MAX_CANDIDATES} candidates; select a scan interval or threshold explicitly")
    if len(peaks) < len(bp):
        return reject("Fewer candidate peaks than expected fragments; missing markers are not permitted")
    candidates = []
    for index, peak in enumerate(peaks):
        if not isinstance(peak, Mapping) or not _finite_number(peak.get("scan")):
            return reject(f"Candidate {index}: finite numeric scan required")
        scan = float(peak["scan"])
        if not 0 <= scan <= _MAX_SCAN:
            return reject(f"Candidate {index}: scan must be in [0, {_MAX_SCAN}]")
        height = peak.get("height")
        if height is not None and not _finite_number(height):
            return reject(f"Candidate {index}: height must be finite when supplied")
        candidates.append({"candidate_index": index, "scan": scan,
                           "height": float(height) if height is not None else None})
    candidates.sort(key=lambda p: p["scan"])
    if any(b["scan"] <= a["scan"] for a, b in zip(candidates, candidates[1:])):
        return reject("Candidate scans must be distinct; duplicates are invalid")
    result["unmatched"] = [dict(peak) for peak in candidates]
    scans = [peak["scan"] for peak in candidates]
    m, n = len(candidates), len(bp)

    # State (a,b) stores the best two paths assigning the current marker to b
    # and the previous marker to a. Future cost depends only on that pair.
    # Keeping two paths per state is sufficient to recover the globally best
    # and second-best distinct full assignments, even if they later merge.
    states = {}
    for a in range(m - n + 1):
        for b in range(a + 1, m - n + 2):
            states[a, b] = [(0.0, (a, b))]
    for marker in range(2, n):
        next_states = {}
        max_next = m - (n - marker) + 1
        for (a, b), paths in states.items():
            for c in range(b + 1, max_next):
                diagnostics["transitions_checked"] += 1
                error = _error_bp(scans[a], scans[b], scans[c],
                                  bp[marker - 2], bp[marker - 1], bp[marker])
                if abs(error) > max_residual_bp:
                    continue
                key = (b, c)
                options = next_states.setdefault(key, [])
                for cost, path in paths:
                    candidate = (cost + error * error, path + (c,))
                    if any(old_path == candidate[1] for _, old_path in options):
                        continue
                    options.append(candidate)
                    options.sort(key=lambda item: (item[0], item[1]))
                    del options[2:]
        states = next_states
        if not states:
            return reject("No complete ordered assignment satisfies the leave-one-out residual limit; "
                          "missing markers, distorted spacing or unsuitable candidates")
    solutions = sorted((item for paths in states.values() for item in paths),
                       key=lambda item: (item[0], item[1]))
    if not solutions:
        return reject("No complete ordered assignment")
    cost, path = solutions[0]
    residuals = [_error_bp(scans[path[k - 1]], scans[path[k]], scans[path[k + 1]],
                           bp[k - 1], bp[k], bp[k + 1]) for k in range(1, n - 1)]
    rms = math.sqrt(cost / (n - 2))
    diagnostics.update({"rms_error_bp": rms,
                        "max_abs_error_bp": max(abs(error) for error in residuals),
                        "holdout_count": n - 2,
                        "endpoint_holdout_available": False,
                        "holdout_residuals_bp": [None] + residuals + [None],
                        "scan_span": scans[path[-1]] - scans[path[0]],
                        "bp_span": bp[-1] - bp[0],
                        "second_best_rms_error_bp": None,
                        "ambiguity_gap_bp": None})
    selected = set(path)
    result["matched"] = [dict(candidates[peak_index], expected_bp=bp[k],
                              holdout_error_bp=([None] + residuals + [None])[k])
                         for k, peak_index in enumerate(path)]
    result["unmatched"] = [dict(peak) for k, peak in enumerate(candidates) if k not in selected]
    if len(solutions) > 1:
        second_cost, second_path = solutions[1]
        second_rms = math.sqrt(second_cost / (n - 2))
        gap = second_rms - rms
        diagnostics.update({"second_best_rms_error_bp": second_rms,
                            "ambiguity_gap_bp": gap,
                            "alternative_match": [dict(candidates[index], expected_bp=bp[k])
                                                  for k, index in enumerate(second_path)]})
        if gap <= ambiguity_margin_bp + 1e-12:
            return reject("Ambiguous ordered assignments: alternative RMS is within "
                          f"{ambiguity_margin_bp:g} bp of the best; calibration withheld")
    anchors = [[scans[index], bp[k]] for k, index in enumerate(path)]
    result["anchors"] = anchors
    result["calibration"] = Calibration(anchors)
    result["status"] = "accepted"
    return result
