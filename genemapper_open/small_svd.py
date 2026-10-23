"""Independent small least-squares solver on absolute-coordinate monomials.

The inspected polynomial caller builds 1,x,x*x and back-substitutes an SVD,
skipping only singular values equal to zero. This implementation instead uses
its own one-sided Jacobi column rotations. Its rotation convergence tolerance
does not truncate singular values: every computed nonzero singular value is
used. Exact arithmetic rank and numerical rank can consequently differ.

There is no centering, basis scaling, ridge, normal-equation solve, dependency
on vendor code, or claim of original JVM arithmetic/iteration equivalence.
"""
import math
from numbers import Real
import sys

SOLVER_ID = 'independent_binary64_one_sided_jacobi_absolute_monomials_v1'
MAX_ROWS = 4096
MAX_COLUMNS = 3
MAX_ABS_MATRIX = 1e16
MAX_ABS_RHS = 1e16
MAX_ABS_X = 1e8
ORTHOGONALITY_TOLERANCE = 8*sys.float_info.epsilon


class SmallSVDError(ValueError):
    """Invalid bounded input or a nonrepresentable intermediate/result."""


class SVDConvergenceError(SmallSVDError):
    """Jacobi columns did not meet the explicit convergence criterion."""


def _finite_number(value, bound, label):
    if not isinstance(value, Real) or isinstance(value, bool):
        raise SmallSVDError(label+' must be a finite real number')
    try:
        number = float(value)
    except (ValueError, OverflowError):
        raise SmallSVDError(label+' cannot be represented') from None
    if not math.isfinite(number) or abs(number) > bound:
        raise SmallSVDError(label+' exceeds the finite input bound')
    return number


def _norm(column):
    value = math.hypot(*column)
    if not math.isfinite(value):
        raise SmallSVDError('Column norm cannot be represented')
    return value


def _correlation(left, right, left_norm, right_norm):
    # Normalization is local to evaluating the rotation angle; it does not
    # precondition or change the supplied least-squares coordinate system.
    value = math.fsum((a/left_norm)*(b/right_norm) for a, b in zip(left, right))
    if not math.isfinite(value):
        raise SmallSVDError('Column correlation cannot be represented')
    return value


def _max_correlation(columns, norms):
    correlations = [abs(_correlation(columns[p], columns[q], norms[p], norms[q]))
                    for p in range(len(columns)) for q in range(p+1, len(columns))
                    if norms[p] != 0 and norms[q] != 0]
    return max(correlations, default=0.0)


def solve_least_squares(matrix, rhs, *, max_sweeps=64):
    """Solve a bounded tall matrix with1..3 columns, returning diagnostics.

    Rows1..4096, rows>=columns, finite |A| and |rhs|<=1e16. No rcond or
    regularization is applied. Zero-norm columns are skipped exactly. All
    computed nonzero singular directions are retained, however small. This
    policy does not promise a stable numerical rank for repeated/nearby x.
    Nonrepresentable rotations/results and failure to converge raise explicit
    SmallSVDError/SVDConvergenceError; callers must not replace them with a fit.
    Input matrix/rhs are not mutated. Coefficients correspond to input columns.
    """
    if type(max_sweeps) is not int or not 1 <= max_sweeps <= 256:
        raise SmallSVDError('max_sweeps must be an integer in1..256')
    if not isinstance(matrix, (list, tuple)) or not 1 <= len(matrix) <= MAX_ROWS:
        raise SmallSVDError('A bounded nonempty row matrix is required')
    if not isinstance(matrix[0], (list, tuple)) or not 1 <= len(matrix[0]) <= MAX_COLUMNS:
        raise SmallSVDError('Matrix must have1..3 columns')
    size = len(matrix[0])
    if len(matrix) < size:
        raise SmallSVDError('This independent solver requires rows>=columns')
    if not isinstance(rhs, (list, tuple)) or len(rhs) != len(matrix):
        raise SmallSVDError('One explicit right-hand value per row is required')
    rows = []
    for row in matrix:
        if not isinstance(row, (list, tuple)) or len(row) != size:
            raise SmallSVDError('Matrix rows must have equal length')
        rows.append([_finite_number(value, MAX_ABS_MATRIX, 'Matrix entry') for value in row])
    values = [_finite_number(value, MAX_ABS_RHS, 'Right-hand value') for value in rhs]
    columns = [[row[j] for row in rows] for j in range(size)]
    vectors = [[1.0 if row == col else 0.0 for row in range(size)] for col in range(size)]
    sweeps = 0
    norms = [_norm(column) for column in columns]
    largest_correlation = _max_correlation(columns, norms)
    while largest_correlation > ORTHOGONALITY_TOLERANCE and sweeps < max_sweeps:
        for p in range(size):
            for q in range(p+1, size):
                left, right = columns[p], columns[q]
                np, nq = _norm(left), _norm(right)
                if np == 0 or nq == 0:
                    continue
                correlation = _correlation(left, right, np, nq)
                if abs(correlation) <= ORTHOGONALITY_TOLERANCE:
                    continue
                # Scale only this two-norm angle calculation, never the basis
                # columns. The small-angle root avoids both cancellation and
                # overflow when one computed singular direction is tiny.
                zeta = (nq/np-np/nq)/(2*correlation)
                if math.isfinite(zeta):
                    if zeta == 0:
                        tangent = 1.0
                    else:
                        # Reciprocal/hypot form of the small quadratic root.
                        root_denominator = abs(zeta)+math.hypot(1.0, zeta)
                        if math.isfinite(root_denominator):
                            tangent = math.copysign(1.0, zeta)/root_denominator
                        else:
                            reciprocal = 1.0/abs(zeta)
                            tangent = math.copysign(reciprocal/(1+math.hypot(1.0, reciprocal)), zeta)
                else:
                    norm_scale = max(np, nq)
                    rp, rq = np/norm_scale, nq/norm_scale
                    difference = rq*rq-rp*rp
                    twice_cross = 2*correlation*rp*rq
                    denominator = difference+math.copysign(math.hypot(difference, twice_cross), difference)
                    tangent = twice_cross/denominator
                if not math.isfinite(tangent) or tangent == 0:
                    raise SmallSVDError('Jacobi angle cannot be represented')
                cosine = 1.0/math.hypot(1.0, tangent)
                sine = cosine*tangent
                columns[p] = [math.fsum((cosine*a, -sine*b)) for a, b in zip(left, right)]
                columns[q] = [math.fsum((sine*a, cosine*b)) for a, b in zip(left, right)]
                vp, vq = vectors[p], vectors[q]
                vectors[p] = [math.fsum((cosine*a, -sine*b)) for a, b in zip(vp, vq)]
                vectors[q] = [math.fsum((sine*a, cosine*b)) for a, b in zip(vp, vq)]
        sweeps += 1
        norms = [_norm(column) for column in columns]
        largest_correlation = _max_correlation(columns, norms)
    if largest_correlation > ORTHOGONALITY_TOLERANCE:
        raise SVDConvergenceError('One-sided Jacobi SVD did not converge within max_sweeps')
    projections = []
    try:
        for column, singular in zip(columns, norms):
            projections.append(0.0 if singular == 0 else
                               math.fsum((value/singular)*target for value, target in zip(column, values))/singular)
        coefficients = [math.fsum(vector[row]*projection for vector, projection in zip(vectors, projections))
                        for row in range(size)]
        residuals = [math.fsum(a*b for a, b in zip(row, coefficients))-target
                     for row, target in zip(rows, values)]
        residual = math.hypot(*residuals)
    except (ValueError, OverflowError):
        raise SmallSVDError('SVD back-substitution or residual cannot be represented') from None
    if not all(math.isfinite(value) for value in coefficients+projections+[residual]):
        raise SmallSVDError('SVD back-substitution or residual is nonfinite')
    nonzero = [value for value in norms if value != 0]
    condition = max(nonzero)/min(nonzero) if nonzero and len(nonzero) == size else None
    if condition is not None and not math.isfinite(condition):
        condition = None
    return {'coefficients': coefficients, 'solver': SOLVER_ID,
            'singular_values': norms, 'computed_nonzero_singular_values': len(nonzero),
            'exact_zero_singular_values': size-len(nonzero), 'sweeps': sweeps,
            'max_column_correlation': largest_correlation,
            'orthogonality_tolerance': ORTHOGONALITY_TOLERANCE,
            'singular_value_policy': 'skip_exact_zero_only_no_rcond',
            'condition_estimate': condition, 'residual_l2': residual,
            'original_runtime_validated': False, 'external_validation': False}


def polynomial_fit(x, y, degree, *, max_sweeps=64):
    """Fit degree1 or2 using absolute monomials; ascending coefficients.

    At least degree+1 and at most4096 pairs, |x|<=1e8, |y|<=1e16. Repeated
    x values are allowed; no rank threshold or rank-based rejection is added.
    Vandermonde rows are [1,x] or [1,x,x*x], preserving absolute coordinates.
    Diagnostics identify this independent solver rather than original SVD.
    """
    if type(degree) is not int or degree not in (1, 2):
        raise SmallSVDError('Only explicit polynomial degree1 or2 is supported')
    if (not isinstance(x, (list, tuple)) or not isinstance(y, (list, tuple))
            or len(x) != len(y) or not degree+1 <= len(x) <= MAX_ROWS):
        raise SmallSVDError('A bounded list/tuple of paired polynomial observations is required')
    coordinates = [_finite_number(value, MAX_ABS_X, 'Polynomial x') for value in x]
    matrix = [[1.0, value] if degree == 1 else [1.0, value, value*value] for value in coordinates]
    result = solve_least_squares(matrix, y, max_sweeps=max_sweeps)
    result.update(polynomial_degree=degree, basis='absolute_scan_monomials_ascending',
                  centered=False, column_preconditioned=False, regularized=False)
    return result
