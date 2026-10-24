"""Independent numerical scan -> bp models; no extrapolation or vendor code.

All curves are checked analytically for monotonicity over their full domain.
Least-squares polynomials use centered/scaled data and Householder QR, rather
than normal equations. The spline has *natural* endpoint conditions; this is
an explicitly specified mathematical model, not a claim of vendor equivalence.
Anchor residuals measure fit consistency and are not an external validation.
"""
from bisect import bisect_right
from copy import deepcopy
import math
from numbers import Real

MODEL_IDS = ("linear", "lsq2", "lsq3", "natural_cubic", "local_rational", "global_southern")
MAX_ANCHORS = 32
MAX_SCAN = 100_000_000
MAX_BP = 1_000_000
_MIN_NORMALIZED_GAP = 1e-12
_QR_RANK_TOLERANCE = 1e-12
_MIN_RATIONAL_Q = 1e-12
_MAX_RATIONAL_Q = 1e12


class CalibrationError(ValueError):
    """Invalid anchors, numerically unresolved fit or nonmonotonic curve."""


def _number(value, label):
    if not isinstance(value, Real) or isinstance(value, bool):
        raise CalibrationError(f"{label} must be a finite number")
    try:
        number = float(value)
    except (ValueError, OverflowError) as exc:
        raise CalibrationError(f"{label} must be a finite number") from exc
    if not math.isfinite(number):
        raise CalibrationError(f"{label} must be a finite number")
    return number


def _validate_anchors(anchors, model):
    if model not in MODEL_IDS:
        raise CalibrationError(f"Unknown calibration model; use {', '.join(MODEL_IDS)}")
    if not isinstance(anchors, (list, tuple)):
        raise CalibrationError("Anchors must be an explicit bounded list or tuple")
    minimum = {"linear": 2, "lsq2": 3, "lsq3": 4, "natural_cubic": 2, "local_rational": 3, "global_southern": 4}[model]
    if not minimum <= len(anchors) <= MAX_ANCHORS:
        raise CalibrationError(f"{model} requires {minimum}..{MAX_ANCHORS} anchors")
    result = []
    for index, pair in enumerate(anchors):
        if not isinstance(pair, (list, tuple)) or len(pair) != 2:
            raise CalibrationError(f"Anchor {index} must contain exactly scan and bp")
        scan = _number(pair[0], f"Anchor {index} scan")
        bp = _number(pair[1], f"Anchor {index} bp")
        if not 0 <= scan <= MAX_SCAN or not 0 < bp <= MAX_BP:
            raise CalibrationError(f"Anchors require scan in [0, {MAX_SCAN}] and bp in (0, {MAX_BP}]")
        result.append((scan, bp))
    if any(b[0] <= a[0] or b[1] <= a[1] for a, b in zip(result, result[1:])):
        raise CalibrationError("Anchor scans and bp must increase strictly in supplied order")
    span = result[-1][0] - result[0][0]
    normalized = [(x - result[0][0]) / span for x, _ in result]
    if any(b - a < _MIN_NORMALIZED_GAP for a, b in zip(normalized, normalized[1:])):
        raise CalibrationError("Numerically unresolved scan spacing; anchors are too closely clustered")
    return tuple(result)


def _polynomial(coefficients, value):
    result = 0.0
    for coefficient in reversed(coefficients):
        result = result * value + coefficient
    return result


def _quadratic_minimum(a, b, c, left, right):
    """Minimum of a*x*x+b*x+c, including its analytic interior extremum."""
    points = [left, right]
    if a > 0:
        vertex = -b / (2 * a)
        if left < vertex < right:
            points.append(vertex)
    return min((a * x + b) * x + c for x in points)


def _quadratic_roots(a, b, c, left, right):
    """Finite derivative roots inside an interval, with stable quadratic q."""
    if a == 0:
        roots = [] if b == 0 else [-c / b]
    else:
        discriminant = b * b - 4 * a * c
        if discriminant < 0:
            return []
        root = math.sqrt(discriminant)
        q = -.5 * (b + math.copysign(root, b))
        roots = [-b / (2 * a)] if q == 0 else [q / a, c / q]
    return sorted(set(x for x in roots if math.isfinite(x) and left <= x <= right))


def _least_squares_qr(matrix, values):
    """Small dense Householder QR; rank checks use scaled matrix columns."""
    rows, columns = len(matrix), len(matrix[0])
    a = [row[:] for row in matrix]
    b = list(values)
    diagonal = []
    for column in range(columns):
        vector = [a[row][column] for row in range(column, rows)]
        norm = math.hypot(*vector)
        if not math.isfinite(norm) or norm <= _QR_RANK_TOLERANCE:
            raise CalibrationError("Singular or numerically rank-deficient polynomial fit")
        alpha = -math.copysign(norm, vector[0])
        vector[0] -= alpha
        vector_norm = math.hypot(*vector)
        vector = [value / vector_norm for value in vector]
        for j in range(column, columns):
            projection = 2 * math.fsum(vector[k] * a[column + k][j] for k in range(len(vector)))
            for k, value in enumerate(vector):
                a[column + k][j] -= projection * value
        projection = 2 * math.fsum(vector[k] * b[column + k] for k in range(len(vector)))
        for k, value in enumerate(vector):
            b[column + k] -= projection * value
        a[column][column] = alpha
        for row in range(column + 1, rows):
            a[row][column] = 0.0
        diagonal.append(abs(alpha))
    if min(diagonal) <= max(diagonal) * _QR_RANK_TOLERANCE:
        raise CalibrationError("Singular or ill-conditioned polynomial fit")
    solution = [0.0] * columns
    for row in range(columns - 1, -1, -1):
        solution[row] = (b[row] - math.fsum(a[row][j] * solution[j]
                                          for j in range(row + 1, columns))) / a[row][row]
    if not all(math.isfinite(value) for value in solution):
        raise CalibrationError("Non-finite polynomial coefficients")
    return solution


class _RationalTriple:
    """Normalized [1/1] rational interpolation of three increasing anchors.

    g(x)=x*q/((1-x)+q*x), where q=v*(1-s)/(s*(1-v)).
    Here s,v are the middle anchor's normalized scan and bp. This follows
    directly from g(0)=0, g(1)=1 and g(s)=v; there are no fitted presets.
    The positive q formulation avoids subtractive cancellation near d=q-1=-1.
    """
    def __init__(self, anchors):
        (self.x0, self.y0), (self.x1, self.y1), (self.x2, self.y2) = anchors
        self.dx, self.dy = self.x2 - self.x0, self.y2 - self.y0
        s, v = (self.x1 - self.x0) / self.dx, (self.y1 - self.y0) / self.dy
        if not 0 < s < 1 or not 0 < v < 1:
            raise CalibrationError("Numerically unresolved rational middle anchor")
        self.q = (v / s) * ((1 - s) / (1 - v))
        if not math.isfinite(self.q) or not _MIN_RATIONAL_Q <= self.q <= _MAX_RATIONAL_Q:
            raise CalibrationError("Numerically unresolved rational denominator or near-pole fit")
        self.denominator_margin = min(1.0, self.q)
        self.minimum_derivative = (self.dy / self.dx) * min(self.q, 1 / self.q)
        if not math.isfinite(self.minimum_derivative) or self.minimum_derivative <= 0:
            raise CalibrationError("Numerically unresolved positive rational derivative")

    def value(self, scan):
        if not self.x0 <= scan <= self.x2:
            raise CalibrationError("Rational evaluation outside supporting triple")
        # Enforce interpolation constraints exactly at the supplied knots.
        if scan == self.x0:
            return self.y0
        if scan == self.x1:
            return self.y1
        if scan == self.x2:
            return self.y2
        x = (scan - self.x0) / self.dx
        denominator = (1 - x) + self.q * x
        if not math.isfinite(denominator) or denominator < self.denominator_margin * (1 - 1e-12):
            raise CalibrationError("Unresolved rational denominator during evaluation")
        return self.y0 + self.dy * ((x * self.q) / denominator)


class _Calibration:
    def __init__(self, anchors, model):
        self._anchors = anchors
        self._scans = tuple(scan for scan, _ in anchors)
        self._bp = tuple(bp for _, bp in anchors)
        self._span = self._scans[-1] - self._scans[0]
        self._u = tuple((scan - self._scans[0]) / self._span for scan in self._scans)
        self._y_center = (self._bp[-1] + self._bp[0]) / 2
        self._y_scale = (self._bp[-1] - self._bp[0]) / 2
        if self._y_scale <= 0:
            raise CalibrationError("Numerically unresolved bp span")
        self._z = tuple((bp - self._y_center) / self._y_scale for bp in self._bp)
        self._model = model
        self._details = {}
        if model == "linear":
            self._minimum_derivative = min((b[1] - a[1]) / (b[0] - a[0])
                                           for a, b in zip(anchors, anchors[1:]))
        elif model in ("lsq2", "lsq3"):
            self._fit_polynomial(int(model[-1]))
        elif model == "local_rational":
            self._fit_local_rational()
        elif model == "global_southern":
            self._fit_global_southern()
        else:
            self._fit_natural_spline()
        if not math.isfinite(self._minimum_derivative) or self._minimum_derivative < 0:
            raise CalibrationError("Calibration is nonmonotonic or numerically unstable within its domain")
        predictions = [self._evaluate(scan) for scan in self._scans]
        if any(not math.isfinite(value) or value <= 0 for value in predictions):
            raise CalibrationError("Calibration produces non-finite or nonpositive fragment sizes")
        residuals = [predicted - observed for predicted, observed in zip(predictions, self._bp)]
        self._diagnostics = {
            "model": model, "monotonic": True, "anchor_count": len(anchors),
            "rms_anchor_error_bp": math.sqrt(math.fsum(error * error for error in residuals) / len(residuals)),
            "max_abs_anchor_error_bp": max(abs(error) for error in residuals),
            "anchor_residuals_bp": residuals, "anchor_predictions_bp": predictions,
            "scan_range": [self._scans[0], self._scans[-1]],
            "anchor_bp_range": [self._bp[0], self._bp[-1]],
            "calibrated_bp_range": [predictions[0], predictions[-1]],
            "minimum_derivative_bp_per_scan": self._minimum_derivative,
            "monotonicity_check": "analytic derivative minima over entire domain",
            "extrapolation": False, "external_validation": False,
            **self._details,
        }

    @property
    def anchors(self):
        return [list(pair) for pair in self._anchors]

    @property
    def diagnostics(self):
        return deepcopy(self._diagnostics)

    def size(self, scan):
        if scan is None:
            return None
        scan = _number(scan, "Query scan")
        if scan < self._scans[0] or scan > self._scans[-1]:
            return None
        value = self._evaluate(scan)
        if not math.isfinite(value):
            raise CalibrationError("Non-finite calibrated fragment size")
        return value

    def standard_map_size(self,scan):
        """Unrounded map value for standard morphology, including exterior scans.

        This separate API does not extend the public allele-sizing domain.
        Local rational/spline exterior branches use the adjacent anchor secant;
        LSQ uses its fitted polynomial. Those model distinctions are observed
        in the original map builders. Interior curves remain our independent
        mathematical implementations; linear is our explicit piecewise model.
        Negative values on secant models preserve the original -1 sentinel.
        """
        scan=_number(scan,'Standard-map query scan')
        if not 0<=scan<=MAX_SCAN:raise CalibrationError('Standard-map query scan outside supported bounds')
        if self._model in ('lsq2','lsq3') or self._scans[0]<=scan<=self._scans[-1]:
            value=self._evaluate(scan)
        else:
            left,right=(self._anchors[:2] if scan<self._scans[0] else self._anchors[-2:])
            slope=(right[1]-left[1])/(right[0]-left[0])
            value=left[1]+slope*(scan-left[0])
        if not math.isfinite(value):raise CalibrationError('Non-finite standard-map size')
        if self._model not in ('lsq2','lsq3') and value<0:return -1.0
        return value

    def _fit_polynomial(self, degree):
        coordinates = [2 * u - 1 for u in self._u]
        matrix = [[t ** power for power in range(degree + 1)] for t in coordinates]
        self._coefficients = _least_squares_qr(matrix, self._z)
        c = self._coefficients
        a, b, constant = (3 * c[3] if degree == 3 else 0), 2 * c[2], c[1]
        minimum = _quadratic_minimum(a, b, constant, -1, 1)
        if not math.isfinite(minimum) or minimum < 0:
            raise CalibrationError("Polynomial calibration is nonmonotonic within its domain")
        self._minimum_derivative = minimum * self._y_scale * 2 / self._span
        roots = _quadratic_roots(a, b, constant, -1, 1)
        self._details = {"degree": degree, "solver": "centered_scaled_householder_qr",
                         "coefficients_scaled": list(c),
                         "derivative_zero_scans": [self._scans[0] + (t + 1) * self._span / 2 for t in roots],
                         "normalization": {"x_left": self._scans[0], "x_span": self._span,
                                           "x_transform": "t = 2 * (scan - x_left) / x_span - 1",
                                           "y_center": self._y_center, "y_scale": self._y_scale}}

    def _fit_natural_spline(self):
        n = len(self._anchors)
        h = [b - a for a, b in zip(self._u, self._u[1:])]
        second = [0.0] * n
        if n > 2:
            # Natural endpoint second derivatives are zero. Solve the remaining
            # strictly diagonally dominant tridiagonal system by Thomas steps.
            diagonal = [2 * (h[i - 1] + h[i]) for i in range(1, n - 1)]
            upper = [h[i] for i in range(1, n - 2)]
            lower = [h[i] for i in range(1, n - 2)]
            rhs = [6 * ((self._z[i + 1] - self._z[i]) / h[i]
                        - (self._z[i] - self._z[i - 1]) / h[i - 1]) for i in range(1, n - 1)]
            for i in range(1, n - 2):
                factor = lower[i - 1] / diagonal[i - 1]
                diagonal[i] -= factor * upper[i - 1]
                rhs[i] -= factor * rhs[i - 1]
            interior = [0.0] * (n - 2)
            for i in range(n - 3, -1, -1):
                if diagonal[i] <= 0 or not math.isfinite(diagonal[i]):
                    raise CalibrationError("Singular natural cubic spline system")
                interior[i] = (rhs[i] - (upper[i] * interior[i + 1] if i < n - 3 else 0)) / diagonal[i]
            second[1:-1] = interior
        self._segments = []
        minimums, roots = [], []
        for i, width in enumerate(h):
            coefficients = [self._z[i],
                            self._z[i + 1] - self._z[i] - width * width * (2 * second[i] + second[i + 1]) / 6,
                            width * width * second[i] / 2,
                            width * width * (second[i + 1] - second[i]) / 6]
            if not all(math.isfinite(value) for value in coefficients):
                raise CalibrationError("Non-finite natural cubic spline coefficients")
            self._segments.append(coefficients)
            a, b, c = 3 * coefficients[3], 2 * coefficients[2], coefficients[1]
            local_minimum = _quadratic_minimum(a, b, c, 0, 1)
            if not math.isfinite(local_minimum) or local_minimum < 0:
                raise CalibrationError(f"Natural cubic calibration is nonmonotonic on segment {i}")
            minimums.append(local_minimum * self._y_scale / (width * self._span))
            roots.extend(self._scans[i] + root * (self._scans[i + 1] - self._scans[i])
                         for root in _quadratic_roots(a, b, c, 0, 1))
        self._minimum_derivative = min(minimums)
        self._details = {"boundary": "natural", "endpoint_second_derivatives": [0.0, 0.0],
                         "derivative_zero_scans": sorted(set(roots)),
                         "segment_count": len(self._segments),
                         "monotonicity_check": "analytic derivative minima on every cubic segment"}

    def _fit_local_rational(self):
        self._triples = [_RationalTriple(self._anchors[i:i + 3])
                         for i in range(len(self._anchors) - 2)]
        self._minimum_derivative = min(triple.minimum_derivative for triple in self._triples)
        self._details = {
            "boundary": "first/last supporting triple; interior overlap average",
            "continuity": "C0; C1 is not imposed or claimed",
            "rational_triple_count": len(self._triples),
            "minimum_denominator_margin": min(triple.denominator_margin for triple in self._triples),
            "maximum_denominator_ratio": max(max(triple.q, 1 / triple.q) for triple in self._triples),
            "minimum_derivative_bound_kind": "conservative lower bound from full-triple analytic minima",
            "monotonicity_check": "positive pole-free rational derivatives; averages preserve positivity",
            "normalized_rational_formula": "g(x)=x*q/((1-x)+q*x), q=v*(1-s)/(s*(1-v))",
        }

    def _fit_global_southern(self):
        # One reciprocal (Southern) curve z = a + b/(u - p) fitted by least squares through all anchors,
        # with the pole p outside the anchor domain [0, 1]; a fragment with anomalous mobility shifts
        # the whole curve slightly instead of bending its local triple (Local Southern).
        u, z = self._u, self._z

        def solve(p):
            w = [1.0 / (x - p) for x in u]
            n = len(u)
            sw, sz, sww, swz = math.fsum(w), math.fsum(z), math.fsum(v * v for v in w), math.fsum(v * y for v, y in zip(w, z))
            det = n * sww - sw * sw
            if not math.isfinite(det) or abs(det) < 1e-300:
                return None
            b = (n * swz - sw * sz) / det
            a = (sz - b * sw) / n
            sse = math.fsum((a + b * v - y) ** 2 for v, y in zip(w, z))
            return sse, a, b

        def pole(side, s):
            return -math.exp(s) if side < 0 else 1.0 + math.exp(s)

        best = None
        for side in (-1, 1):
            grid = [-12 + 24 * i / 240 for i in range(241)]
            values = [(solve(pole(side, s)), s) for s in grid]
            values = [(r, s) for r, s in values if r is not None]
            if not values:
                continue
            (r0, s0) = min(values, key=lambda item: item[0][0])
            lo, hi = max(-12.0, s0 - 0.1), min(12.0, s0 + 0.1)
            g = (math.sqrt(5) - 1) / 2
            c, d = hi - g * (hi - lo), lo + g * (hi - lo)
            for _ in range(80):
                rc, rd = solve(pole(side, c)), solve(pole(side, d))
                if rc is None or rd is None:
                    break
                if rc[0] <= rd[0]:
                    hi, d = d, c
                    c = hi - g * (hi - lo)
                else:
                    lo, c = c, d
                    d = lo + g * (hi - lo)
            s = (lo + hi) / 2
            r = solve(pole(side, s)) or r0
            if best is None or r[0] < best[0][0]:
                best = (r, pole(side, s if r is not r0 else s0))
        # limit of a pole at infinity: a straight line (exactly linear mobility)
        n = len(u)
        su, sz, suu, suz = math.fsum(u), math.fsum(z), math.fsum(x * x for x in u), math.fsum(x * y for x, y in zip(u, z))
        slope = (n * suz - su * sz) / (n * suu - su * su)
        icpt = (sz - slope * su) / n
        line_sse = math.fsum((icpt + slope * x - y) ** 2 for x, y in zip(u, z))
        if best is None or line_sse <= best[0][0] * (1 + 1e-9) + 1e-24:
            if not slope > 0:
                raise CalibrationError("Global Southern calibration is nonmonotonic within its domain")
            self._gs = (icpt, slope, None)
            self._minimum_derivative = slope * self._y_scale / self._span
            sse, a, b, p = line_sse, icpt, slope, None
        else:
            (sse, a, b), p = best
            if not (math.isfinite(a) and math.isfinite(b)) or b >= 0:
                raise CalibrationError("Global Southern calibration is nonmonotonic within its domain")
            self._gs = (a, b, p)
            far = max(abs(0 - p), abs(1 - p))
            self._minimum_derivative = (-b / (far * far)) * self._y_scale / self._span
        self._details = {"formula": "bp = c + m/(scan - s0) (least squares over all fragments)",
                         "pole_scan": None if p is None else self._scans[0] + p * self._span,
                         "c_bp": self._y_center + self._y_scale * a,
                         "m_bp_scan": None if p is None else self._y_scale * b * self._span,
                         "limit": "linear (pole at infinity)" if p is None else None,
                         "residual_sum_squares_bp2": sse * self._y_scale ** 2,
                         "solver": "linear least squares in (c, m) with golden-section search of the pole",
                         "monotonicity_check": "derivative -m/(scan-s0)^2 positive on the whole domain"}

    def _evaluate(self, scan):
        if self._model == "linear":
            i = min(bisect_right(self._scans, scan) - 1, len(self._scans) - 2)
            x0, y0 = self._anchors[i]
            x1, y1 = self._anchors[i + 1]
            return y0 + (scan - x0) / (x1 - x0) * (y1 - y0)
        if self._model == "local_rational":
            i = min(bisect_right(self._scans, scan) - 1, len(self._scans) - 2)
            left = self._triples[max(0, i - 1)]
            right = self._triples[min(i, len(self._triples) - 1)]
            left_value = left.value(scan)
            return left_value if left is right else left_value + (right.value(scan) - left_value) / 2
        u = (scan - self._scans[0]) / self._span
        if self._model == "global_southern":
            a, b, p = self._gs
            return self._y_center + self._y_scale * (a + (b * u if p is None else b / (u - p)))
        if self._model in ("lsq2", "lsq3"):
            z = _polynomial(self._coefficients, 2 * u - 1)
        else:
            i = min(bisect_right(self._u, u) - 1, len(self._u) - 2)
            local = (u - self._u[i]) / (self._u[i + 1] - self._u[i])
            z = _polynomial(self._segments[i], local)
        return self._y_center + self._y_scale * z


def build_calibration(anchors, model="linear"):
    """Build a finite, monotonic, bounded scan->bp calibration, or raise.

    Anchor order is explicit and must already increase in scan and size; no
    sorting or removal is performed. There are 2..32 anchors (3 minimum for
    lsq2/local_rational, 4 for lsq3). Scans are in [0, 1e8], bp in (0, 1e6]. Adjacent scan
    positions must be separated by at least 1e-12 of the whole scan span to
    avoid numerically unresolved anchor clusters. ``linear`` is piecewise
    linear interpolation; ``lsq2`` and ``lsq3`` are least-squares polynomial
    fits, and ``natural_cubic`` interpolates with zero endpoint curvature.
    ``local_rational`` uses normalized three-anchor [1/1] rational curves:
    the first/last interval uses the first/last triple, and inner intervals
    average neighboring triples. It is C0; derivative continuity is not imposed.
    Rational denominator ratios outside [1e-12, 1e12] are rejected as unresolved.

    Polynomial and spline derivatives are verified analytically, including
    interior derivative extrema and derivative zeros. A curve with negative
    derivative anywhere is rejected, even if all anchors themselves increase.
    Zero isolated derivatives are allowed for a still-increasing curve.
    Rational derivatives are analytically positive throughout supporting triples;
    averaging preserves positivity, and their diagnostic minimum is a conservative
    lower bound. No manufacturer algorithm equivalence is asserted.
    All models withhold sizes outside the closed anchor scan domain. Model
    fit residuals do not establish that the anchors are biologically correct.
    """
    return _Calibration(_validate_anchors(anchors, model), model)
