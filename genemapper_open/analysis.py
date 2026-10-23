"""Exploratory peak detection and manual piecewise linear size calibration.

These routines do not implement or claim equivalence to GeneMapper.
"""
from bisect import bisect_right
import math
from statistics import median


def find_peaks(values, threshold=100.0, min_distance=8, baseline_window=101):
    """Subtract interpolated block medians and detect plateau-aware maxima.

    Height uses median-corrected signal; area is positive corrected
    signal between the nearest non-positive points (capped at 100 scans).
    The baseline uses linear interpolation between block centers to avoid
    discontinuities. Fixed blocks and area cap are research simplifications.
    """
    if not math.isfinite(threshold) or threshold < 0:
        raise ValueError("Threshold must be finite and nonnegative")
    if min_distance < 1 or baseline_window < 3:
        raise ValueError("Distance >= 1 and baseline window >= 3 required")
    if any(not math.isfinite(v) for v in values):
        raise ValueError("Non-finite trace values")
    blocks = []
    for start in range(0, len(values), baseline_window):
        block = values[start:start + baseline_window]
        blocks.append((start + (len(block) - 1) / 2, median(block)))
    corrected = []
    segment = 0
    for scan, value in enumerate(values):
        while segment + 1 < len(blocks) and scan > blocks[segment + 1][0]:
            segment += 1
        x0, y0 = blocks[segment]
        if segment + 1 < len(blocks) and scan > x0:
            x1, y1 = blocks[segment + 1]
            baseline = y0 + (scan - x0) / (x1 - x0) * (y1 - y0)
        else:
            baseline = y0
        corrected.append(value - baseline)
    candidates = []
    i = 1
    while i < len(corrected) - 1:
        if corrected[i] >= threshold and corrected[i] > corrected[i - 1]:
            end = i
            while end + 1 < len(corrected) and corrected[end + 1] == corrected[i]:
                end += 1
            if end + 1 < len(corrected) and corrected[end] > corrected[end + 1]:
                candidates.append((i + end) // 2)
            i = end + 1
        else:
            i += 1
    # Select taller candidates first, then return in scan order.
    chosen = []
    for scan in sorted(candidates, key=lambda p: (-corrected[p], p)):
        if all(abs(scan - previous) >= min_distance for previous in chosen):
            chosen.append(scan)
    peaks = []
    for scan in sorted(chosen):
        left = right = scan
        while left > max(0, scan - 100) and corrected[left - 1] > 0:
            left -= 1
        while right < min(len(values) - 1, scan + 100) and corrected[right + 1] > 0:
            right += 1
        peaks.append({"scan": scan, "height": corrected[scan],
                      "raw_height": values[scan], "area": sum(max(0, v) for v in corrected[left:right + 1]),
                      "width_scans": right - left + 1})
    return peaks


class Calibration:
    """Manual scan -> bp anchors; interpolate only, never extrapolate."""
    def __init__(self, anchors):
        self.anchors = sorted((float(x), float(y)) for x, y in anchors)
        if len(self.anchors) < 2 or any(not math.isfinite(v) for a in self.anchors for v in a):
            raise ValueError("At least two finite scan/bp anchors required")
        if any(x < 0 or y <= 0 for x, y in self.anchors):
            raise ValueError("Scans must be nonnegative and bp positive")
        if any(b[0] <= a[0] or b[1] <= a[1] for a, b in zip(self.anchors, self.anchors[1:])):
            raise ValueError("Anchor scans and bp must increase strictly")
        self.scans = [a[0] for a in self.anchors]

    def size(self, scan):
        if not self.scans[0] <= scan <= self.scans[-1]:
            return None
        i = min(bisect_right(self.scans, scan) - 1, len(self.anchors) - 2)
        (x0, y0), (x1, y1) = self.anchors[i:i + 2]
        return y0 + (scan - x0) * (y1 - y0) / (x1 - x0)


def parse_anchors(text):
    """Parse '100:75,200:150' or one scan:bp pair per line."""
    if not text.strip():
        return None
    anchors = []
    for item in text.replace("\n", ",").split(","):
        if item.strip():
            parts = item.split(":")
            if len(parts) != 2:
                raise ValueError("Use scan:bp pairs separated by commas")
            anchors.append(tuple(map(float, parts)))
    return Calibration(anchors)
