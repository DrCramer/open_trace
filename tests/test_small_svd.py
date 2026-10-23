"""Analytic and exact-rational fixtures for the independent Jacobi solver."""
from fractions import Fraction
import json
import math
import unittest

from genemapper_open.small_svd import (SOLVER_ID, SmallSVDError,
    SVDConvergenceError, ORTHOGONALITY_TOLERANCE, polynomial_fit,
    solve_least_squares)


def _exact_orthogonal_projection(xs, ys, degree):
    """Exact-rational Gram-Schmidt oracle, independent of Jacobi rotations.

    No floating normal-equation solve or production helper is used. Basis
    vectors and their coefficient transformations stay exact Fractions.
    """
    observations = [Fraction(value) for value in ys]
    orthogonal, transforms, norms = [], [], []
    for power in range(degree+1):
        column = [Fraction(value)**power for value in xs]
        transform = [Fraction(i == power) for i in range(degree+1)]
        for previous, mapping, norm in zip(orthogonal, transforms, norms):
            projection = sum(a*b for a, b in zip(column, previous))/norm
            column = [a-projection*b for a, b in zip(column, previous)]
            transform = [a-projection*b for a, b in zip(transform, mapping)]
        orthogonal.append(column)
        transforms.append(transform)
        norms.append(sum(a*a for a in column))
    weights = [sum(a*b for a, b in zip(column, observations))/norm
               for column, norm in zip(orthogonal, norms)]
    return [sum(weight*transform[index] for weight, transform in zip(weights, transforms))
            for index in range(degree+1)]


def _evaluate(coefficients, x):
    return math.fsum(value*x**power for power, value in enumerate(coefficients))


class SmallSVDTests(unittest.TestCase):
    def test_analytic_overdetermined_line(self):
        result = polynomial_fit([0,1,2,3,4], [1,2,2,4,5], 1)
        for actual, expected in zip(result['coefficients'], [0.8,1.0]):
            self.assertAlmostEqual(actual, expected, delta=2e-14)
        self.assertAlmostEqual(result['residual_l2'], math.sqrt(0.8), delta=2e-14)
        self.assertEqual(result['solver'], SOLVER_ID)
        self.assertEqual(result['basis'], 'absolute_scan_monomials_ascending')
        self.assertFalse(result['centered'])
        self.assertFalse(result['column_preconditioned'])
        self.assertFalse(result['regularized'])

    def test_quadratic_three_points_near4000(self):
        xs, ys = [3999,4000,4001], [3,1,3]
        expected = _exact_orthogonal_projection(xs, ys, 2)
        self.assertEqual(expected, [Fraction(32000001),Fraction(-16000),Fraction(2)])
        result = polynomial_fit(xs, ys, 2)
        for actual, exact in zip(result['coefficients'], expected):
            self.assertAlmostEqual(actual/float(exact), 1.0, delta=1e-8)
        for x, y in zip(xs, ys):
            self.assertAlmostEqual(_evaluate(result['coefficients'], x), y, delta=2e-7)
        a, b, c = result['coefficients']
        self.assertAlmostEqual(-b/(2*c), 4000, delta=2e-7)
        self.assertGreater(result['condition_estimate'], 1e14)

    def test_five_point_quadratic_known_absolute_coefficients(self):
        xs = [3998,3999,4000,4001,4002]
        ys = [1005,1002,1001,1002,1005]
        expected = [16001001,-8000,1]
        result = polynomial_fit(xs, ys, 2)
        for actual, exact in zip(result['coefficients'], expected):
            self.assertAlmostEqual(actual/float(exact), 1.0, delta=2e-9)
        self.assertLess(result['residual_l2'], 2e-7)

    def test_noisy_six_point_fixture_against_exact_rational_projection(self):
        xs = [3997,3998,3999,4000,4001,4002]
        ys = [3,1,4,1,5,9]
        expected = _exact_orthogonal_projection(xs, ys, 2)
        result = polynomial_fit(xs, ys, 2)
        for x in xs+[Fraction(7999,2)]:
            exact = sum(value*Fraction(x)**power for power, value in enumerate(expected))
            self.assertAlmostEqual(_evaluate(result['coefficients'], float(x)), float(exact), delta=2e-6)
        residual = math.sqrt(float(sum((sum(value*Fraction(x)**power for power,value in enumerate(expected))-y)**2
                                       for x,y in zip(xs,ys))))
        self.assertAlmostEqual(result['residual_l2'], residual, delta=2e-6)

    def test_repeated_observations_preserve_the_least_squares_problem(self):
        result = polynomial_fit([1,1,2,2,3,3], [1,3,5,7,11,13], 2)
        for actual, exact in zip(result['coefficients'], [0,1,1]):
            self.assertAlmostEqual(actual, exact, delta=1e-12)
        self.assertAlmostEqual(result['residual_l2'], math.sqrt(6), delta=1e-12)

    def test_exact_zero_directions_and_singular_repeated_x(self):
        result = polynomial_fit([1,1,1], [1,2,3], 2)
        self.assertEqual(result['exact_zero_singular_values'], 2)
        for value in result['coefficients']:
            self.assertAlmostEqual(value, 2/3, delta=2e-14)
        linear = polynomial_fit([2,2,2], [1,2,3], 1)
        self.assertEqual(linear['exact_zero_singular_values'], 1)
        for actual, exact in zip(linear['coefficients'], [.4,.8]):
            self.assertAlmostEqual(actual, exact, delta=2e-14)
        zeros = polynomial_fit([0,0,0], [1,2,3], 2)
        self.assertEqual(zeros['exact_zero_singular_values'], 2)
        self.assertEqual(zeros['coefficients'][1:], [0,0])
        self.assertAlmostEqual(zeros['coefficients'][0], 2, delta=2e-14)

    def test_nonzero_tiny_singular_values_are_not_truncated(self):
        result = solve_least_squares([[1,0],[0,1e-300]], [2,3e-300])
        self.assertEqual(result['singular_values'], [1,1e-300])
        self.assertEqual(result['exact_zero_singular_values'], 0)
        self.assertAlmostEqual(result['coefficients'][0], 2, delta=1e-14)
        self.assertAlmostEqual(result['coefficients'][1], 3, delta=1e-14)
        self.assertEqual(result['singular_value_policy'], 'skip_exact_zero_only_no_rcond')

    def test_zero_matrix_returns_zero_coefficients(self):
        result = solve_least_squares([[0,0],[0,0]], [3,4])
        self.assertEqual(result['coefficients'], [0,0])
        self.assertEqual(result['exact_zero_singular_values'], 2)
        self.assertEqual(result['residual_l2'], 5)

    def test_convergence_budget_failure_is_explicit(self):
        with self.assertRaises(SVDConvergenceError):
            polynomial_fit([3998,3999,4000,4001,4002], [1005,1002,1001,1002,1005], 2, max_sweeps=1)
        result = polynomial_fit([0,1,2], [2,4,8], 2)
        self.assertLessEqual(result['max_column_correlation'], ORTHOGONALITY_TOLERANCE)

    def test_finite_bounds_and_shapes(self):
        for xs, ys, degree in ((None,[1,2],1),([0,1],[2],1),([0,1],[2,3],True),
                               ([0,1],[2,3],3),([0,float('nan')],[2,3],1),
                               ([0,1],[2,float('inf')],1),([0,100000001],[2,3],1),
                               ([True,1],[2,3],1)):
            with self.assertRaises(SmallSVDError):
                polynomial_fit(xs, ys, degree)
        for matrix, rhs in (([],[]),([[1,2]],[3]),([[1],[2,3]],[2,3]),([[1]],[True]),([[1e17]],[1])):
            with self.assertRaises(SmallSVDError):
                solve_least_squares(matrix, rhs)
        for budget in (0,257,True):
            with self.assertRaises(SmallSVDError):
                solve_least_squares([[1]],[1],max_sweeps=budget)

    def test_inputs_unchanged_and_diagnostics_are_strict_json(self):
        matrix, rhs = [[1,2],[3,4],[5,7]], [3,2,1]
        snapshot = [list(row) for row in matrix], list(rhs)
        result = solve_least_squares(matrix, rhs)
        self.assertEqual((matrix,rhs), snapshot)
        json.dumps(result, allow_nan=False)
        self.assertFalse(result['original_runtime_validated'])
        self.assertFalse(result['external_validation'])


if __name__ == '__main__':
    unittest.main()
