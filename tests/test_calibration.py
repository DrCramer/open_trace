"""Numerical model checks against analytic functions and independent invariants."""
import itertools
import json
import math
import unittest

from genemapper_open.calibration import (CalibrationError, MAX_ANCHORS, MODEL_IDS,
                                       build_calibration)


class CalibrationModelsTests(unittest.TestCase):
    def test_linear_piecewise_known_values(self):
        calibration = build_calibration([(100, 50), (300, 150), (500, 350)])
        self.assertEqual(calibration.size(200), 100)
        self.assertEqual(calibration.size(400), 250)
        self.assertEqual(calibration.diagnostics["max_abs_anchor_error_bp"], 0)

    def test_all_models_closed_domain_and_no_extrapolation(self):
        anchors = [(100, 50), (200, 100), (300, 150), (400, 200), (500, 250)]
        for model in MODEL_IDS:
            with self.subTest(model=model):
                calibration = build_calibration(anchors, model)
                self.assertAlmostEqual(calibration.size(100), 50)
                self.assertAlmostEqual(calibration.size(500), 250)
                self.assertIsNone(calibration.size(99))
                self.assertIsNone(calibration.size(501))
                self.assertIsNone(calibration.size(-100))
                self.assertIsNone(calibration.size(None))

    def test_quadratic_exact_overdetermined_fit(self):
        function = lambda x: 50 + 2 * x + .2 * x * x
        anchors = [(x, function(x)) for x in (0, 1, 2, 4, 5, 8, 10)]
        calibration = build_calibration(anchors, "lsq2")
        for x in (0, .25, 1.5, 4.3, 7.25, 9.5, 10):
            self.assertAlmostEqual(calibration.size(x), function(x), places=10)
        self.assertLess(calibration.diagnostics["max_abs_anchor_error_bp"], 1e-10)
        self.assertEqual(calibration.diagnostics["degree"], 2)
        self.assertEqual(calibration.diagnostics["solver"], "centered_scaled_householder_qr")

    def test_cubic_exact_overdetermined_fit(self):
        function = lambda x: 75 + 3 * x + .1 * x * x + .01 * x ** 3
        anchors = [(x, function(x)) for x in (0, 1, 2, 3, 5, 7, 9, 10)]
        calibration = build_calibration(anchors, "lsq3")
        for x in (0, .2, 2.7, 5.5, 8.9, 10):
            self.assertAlmostEqual(calibration.size(x), function(x), places=10)
        self.assertLess(calibration.diagnostics["rms_anchor_error_bp"], 1e-10)
        self.assertGreater(calibration.diagnostics["minimum_derivative_bp_per_scan"], 2.9)

    def test_centering_large_scan_offset_avoids_raw_vandermonde_instability(self):
        offset = 99_999_990
        function = lambda delta: 100 + 3 * delta + .2 * delta ** 2 + .01 * delta ** 3
        anchors = [(offset + delta, function(delta)) for delta in (0, 1, 2, 3, 5, 7, 9, 10)]
        calibration = build_calibration(anchors, "lsq3")
        for delta in (0, .5, 2.5, 5.5, 9.25, 10):
            self.assertAlmostEqual(calibration.size(offset + delta), function(delta), places=9)
        self.assertLess(calibration.diagnostics["max_abs_anchor_error_bp"], 1e-9)

    def test_scaled_large_bp_and_short_scan_span(self):
        origin, step = 1000, .001
        anchors = [(origin + i * step, 900000 + 100 * i + .5 * i * i) for i in range(8)]
        calibration = build_calibration(anchors, "lsq2")
        self.assertAlmostEqual(calibration.size(origin + 3.5 * step), 900356.125, places=5)
        self.assertLess(calibration.diagnostics["max_abs_anchor_error_bp"], 1e-5)

    def test_noisy_least_squares_has_honest_nonzero_anchor_errors(self):
        noise = [.1, -.2, .15, -.1, .05, .15, -.05, -.1, .1]
        truth = lambda x: 50 + 20 * x + .3 * x * x
        anchors = [(x, truth(x) + noise[x]) for x in range(9)]
        calibration = build_calibration(anchors, "lsq2")
        diagnostics = calibration.diagnostics
        self.assertGreater(diagnostics["rms_anchor_error_bp"], .05)
        self.assertLess(diagnostics["rms_anchor_error_bp"], .2)
        self.assertAlmostEqual(calibration.size(4.5), truth(4.5), delta=.1)
        expected_errors = [calibration.size(x) - y for x, y in anchors]
        self.assertEqual(diagnostics["anchor_residuals_bp"], expected_errors)
        expected_rms = math.sqrt(sum(error * error for error in expected_errors) / len(anchors))
        self.assertAlmostEqual(diagnostics["rms_anchor_error_bp"], expected_rms)

    def test_natural_spline_matches_independent_three_node_solution(self):
        calibration = build_calibration([(0, 1), (1, 2), (2, 4)], "natural_cubic")
        # The exact natural solution has node second derivatives [0, 1.5, 0].
        self.assertAlmostEqual(calibration.size(.5), 1.40625, places=12)
        self.assertAlmostEqual(calibration.size(1.5), 2.90625, places=12)
        self.assertEqual(calibration.diagnostics["boundary"], "natural")
        self.assertEqual(calibration.diagnostics["endpoint_second_derivatives"], [0, 0])

    def test_two_node_natural_spline_is_linear(self):
        calibration = build_calibration([(10, 100), (20, 200)], "natural_cubic")
        self.assertAlmostEqual(calibration.size(12.5), 125)
        self.assertAlmostEqual(calibration.diagnostics["minimum_derivative_bp_per_scan"], 10)

    def test_natural_spline_interpolates_all_anchors_on_irregular_grid(self):
        anchors = [(0, 50), (11, 55), (50, 80), (120, 140), (160, 180), (220, 250)]
        calibration = build_calibration(anchors, "natural_cubic")
        for x, y in anchors:
            self.assertAlmostEqual(calibration.size(x), y, places=10)
        values = [calibration.size(i) for i in range(221)]
        self.assertTrue(all(b > a for a, b in zip(values, values[1:])))
        self.assertLess(calibration.diagnostics["max_abs_anchor_error_bp"], 1e-10)

    def test_natural_spline_continuity_and_natural_boundary_numeric_crosscheck(self):
        calibration = build_calibration([(0, 1), (1, 2), (2, 4)], "natural_cubic")
        h = .001
        left_second = (calibration.size(0) - 2 * calibration.size(h) + calibration.size(2 * h)) / (h * h)
        right_second = (calibration.size(2) - 2 * calibration.size(2 - h) + calibration.size(2 - 2 * h)) / (h * h)
        self.assertAlmostEqual(left_second, 0, delta=.002)
        self.assertAlmostEqual(right_second, 0, delta=.002)
        left_first = (calibration.size(1) - calibration.size(1 - h)) / h
        right_first = (calibration.size(1 + h) - calibration.size(1)) / h
        self.assertAlmostEqual(left_first, right_first, delta=.002)

    def test_increasing_anchors_do_not_guarantee_monotone_quadratic(self):
        # y=100+100*(x-.1)^2 decreases on [0,.1], despite increasing nodes.
        anchors = [(x, 100 + 100 * (x - .1) ** 2) for x in (0, .3, 1)]
        self.assertTrue(all(b[1] > a[1] for a, b in zip(anchors, anchors[1:])))
        with self.assertRaisesRegex(CalibrationError, "nonmonotonic"):
            build_calibration(anchors, "lsq2")

    def test_cubic_negative_derivative_between_increasing_anchors_rejected(self):
        function = lambda x: 100 + 1000 * (x ** 3 / 3 - .5 * x * x + .24 * x)
        anchors = [(x, function(x)) for x in (0, .3, .7, 1)]
        self.assertTrue(all(b[1] > a[1] for a, b in zip(anchors, anchors[1:])))
        with self.assertRaisesRegex(CalibrationError, "nonmonotonic"):
            build_calibration(anchors, "lsq3")

    def test_analytic_check_rejects_narrow_dip_that_coarse_sampling_misses(self):
        center = .512345
        function = lambda x: 100 + x ** 3 / 3 - center * x * x + (center * center - 1e-10) * x
        anchors = [(x, function(x)) for x in (0, .25, .75, 1)]
        self.assertTrue(all(b[1] > a[1] for a, b in zip(anchors, anchors[1:])))
        with self.assertRaisesRegex(CalibrationError, "nonmonotonic"):
            build_calibration(anchors, "lsq3")

    def test_increasing_anchors_but_overshooting_natural_spline_rejected(self):
        anchors = [(0, 1), (1, 2), (2, 2.1), (3, 3)]
        with self.assertRaisesRegex(CalibrationError, "nonmonotonic"):
            build_calibration(anchors, "natural_cubic")
        # The same anchors are valid for the non-overshooting linear model.
        self.assertEqual(build_calibration(anchors).size(1.5), 2.05)

    def test_diagnostics_json_safe_and_defensive_copies(self):
        anchors = [(0, 50), (100, 100), (200, 150), (300, 200), (400, 250)]
        for model in MODEL_IDS:
            with self.subTest(model=model):
                calibration = build_calibration(anchors, model)
                diagnostics = calibration.diagnostics
                decoded = json.loads(json.dumps(diagnostics, allow_nan=False))
                self.assertEqual(decoded["model"], model)
                self.assertTrue(decoded["monotonic"])
                self.assertFalse(decoded["extrapolation"])
                diagnostics["scan_range"][0] = -100
                self.assertEqual(calibration.diagnostics["scan_range"][0], 0)
                external = calibration.anchors
                external[0][1] = -1
                self.assertEqual(calibration.size(0), 50)

    def test_invalid_query_values(self):
        calibration = build_calibration([(100, 50), (200, 100)])
        for value in (math.nan, math.inf, -math.inf, True, False, "100", [], {}, 10 ** 1000):
            with self.subTest(value=repr(value)), self.assertRaises(CalibrationError):
                calibration.size(value)

    def test_models_have_correct_minimum_anchor_counts(self):
        for model, count in (("linear", 2), ("natural_cubic", 2), ("lsq2", 3), ("lsq3", 4),
                             ("local_rational", 3)):
            anchors = [(i, 50 + 10 * i) for i in range(count)]
            self.assertEqual(build_calibration(anchors, model).diagnostics["anchor_count"], count)
            with self.assertRaises(CalibrationError):
                build_calibration(anchors[:-1], model)

    def test_maximum_anchors_and_bounds(self):
        anchors = [(i, 50 + i * 2) for i in range(MAX_ANCHORS)]
        for model in MODEL_IDS:
            with self.subTest(model=model):
                self.assertEqual(len(build_calibration(anchors, model).anchors), MAX_ANCHORS)
                with self.assertRaises(CalibrationError):
                    build_calibration(anchors + [(MAX_ANCHORS, 50 + MAX_ANCHORS * 2)], model)

    def test_invalid_anchor_values_and_order(self):
        invalid_sets = [[], [(1, 2)], [(1, 2), (1, 3)], [(2, 2), (1, 3)],
                        [(1, 3), (2, 2)], [(1, 2), (2, 2)], [(-1, 2), (2, 3)],
                        [(1, 0), (2, 3)], [(1, 2), (math.nan, 3)], [(1, 2), (2, math.inf)],
                        [(1, 2), (100000001, 3)], [(1, 2), (2, 1000001)],
                        [(True, 2), (2, 3)], [(1, 2), (2, False)], [("1", 2), (2, 3)],
                        [(1, 2, 3), (2, 3)], [None, (2, 3)], [(1, 2), (2, 10 ** 1000)]]
        for anchors in invalid_sets:
            with self.subTest(anchors=repr(anchors)), self.assertRaises(CalibrationError):
                build_calibration(anchors)

    def test_unsupported_model_explicit_error(self):
        for model in ("cubic", "vendor", "local_southern", None, []):
            with self.subTest(model=model), self.assertRaisesRegex(CalibrationError, "Unknown"):
                build_calibration([(0, 1), (1, 2)], model)

    def test_unbounded_iterables_are_not_consumed(self):
        with self.assertRaises(CalibrationError):
            build_calibration(itertools.repeat((1, 2)))

    def test_numerically_unresolved_scan_spacing_rejected(self):
        for model in MODEL_IDS:
            with self.subTest(model=model), self.assertRaisesRegex(CalibrationError, "unresolved"):
                build_calibration([(0, 1), (1e-15, 2), (2e-15, 3), (1, 4)], model)

    def test_singular_clustered_cubic_fit_rejected_by_rank_check(self):
        with self.assertRaisesRegex(CalibrationError, "Singular|rank|condition"):
            build_calibration([(0, 1), (1e-10, 2), (2e-10, 3), (1, 4)], "lsq3")


class LocalRationalTests(unittest.TestCase):
    def test_known_single_rational_function_recovered_by_all_triples(self):
        truth = lambda x: 30 + 400 * x * 1.7 / (1 + .7 * x)
        anchors = [(100 + 500 * x, truth(x)) for x in (0, .1, .3, .7, 1)]
        calibration = build_calibration(anchors, "local_rational")
        for i in range(101):
            self.assertAlmostEqual(calibration.size(100 + 5 * i), truth(i / 100), places=10)
        self.assertEqual(calibration.diagnostics["rational_triple_count"], 3)
        self.assertEqual(calibration.diagnostics["max_abs_anchor_error_bp"], 0)

    def test_hand_computed_first_last_and_averaged_inner_interval(self):
        calibration = build_calibration([(0, 10), (2, 30), (5, 50), (9, 100)], "local_rational")
        # First triple q=3/2; at scan1 its normalized value is3/11.
        self.assertAlmostEqual(calibration.size(1), 230 / 11, places=12)
        # At scan3, left triple gives490/13, right(q=8/15) gives250/7.
        # The inner interval uses their arithmetic mean3340/91.
        self.assertAlmostEqual(calibration.size(3), 3340 / 91, places=12)
        self.assertGreater(abs(calibration.size(3) - 490 / 13), .9)
        self.assertGreater(abs(calibration.size(3) - 250 / 7), .9)
        self.assertAlmostEqual(calibration.size(7), 70, places=12)

    def test_three_anchor_model_and_minimum_count(self):
        calibration = build_calibration([(0, 10), (2, 30), (5, 50)], "local_rational")
        self.assertAlmostEqual(calibration.size(3), 490 / 13)
        with self.assertRaisesRegex(CalibrationError, "3..32"):
            build_calibration([(0, 10), (5, 50)], "local_rational")

    def test_positive_affine_scan_and_bp_coordinate_invariance(self):
        anchors = [(0, 10), (2, 30), (5, 50), (9, 100)]
        baseline = build_calibration(anchors, "local_rational")
        for x_shift, x_scale, y_shift, y_scale in ((99_999_100, 100, 50, 3),
                                                  (.1, .001, 1000, .2)):
            transformed = [(x_shift + x_scale * x, y_shift + y_scale * y) for x, y in anchors]
            calibration = build_calibration(transformed, "local_rational")
            for scan in (.1, 1, 3, 4, 6, 8, 8.9):
                with self.subTest(scan=scan, x_shift=x_shift):
                    self.assertAlmostEqual(calibration.size(x_shift + x_scale * scan),
                                           y_shift + y_scale * baseline.size(scan), places=9)

    def test_pure_linear_degenerate_rational_case(self):
        anchors = [(0, 50), (7, 64), (25, 100), (60, 170), (100, 250)]
        calibration = build_calibration(anchors, "local_rational")
        for scan in (0, .5, 3, 12, 40, 80, 100):
            self.assertAlmostEqual(calibration.size(scan), 50 + 2 * scan, places=12)
        self.assertAlmostEqual(calibration.diagnostics["minimum_derivative_bp_per_scan"], 2)

    def test_analytic_positive_derivative_bound_and_monotone_values(self):
        calibration = build_calibration([(0, 10), (2, 30), (5, 50), (9, 100)], "local_rational")
        diagnostics = calibration.diagnostics
        self.assertTrue(diagnostics["monotonic"])
        self.assertIn("conservative lower bound", diagnostics["minimum_derivative_bound_kind"])
        # Triple1 bound=(40/5)*min(3/2,2/3)=16/3.
        # Triple2 bound=(70/7)*min(8/15,15/8)=16/3.
        self.assertAlmostEqual(diagnostics["minimum_derivative_bp_per_scan"], 16 / 3)
        self.assertGreater(diagnostics["minimum_denominator_margin"], 0)
        values = [calibration.size(i / 100) for i in range(901)]
        self.assertTrue(all(b > a for a, b in zip(values, values[1:])))
        bound = diagnostics["minimum_derivative_bp_per_scan"]
        self.assertTrue(all((b - a) / .01 >= bound - 1e-9 for a, b in zip(values, values[1:])))

    def test_c0_anchor_constraints_and_no_extrapolation(self):
        anchors = [(0, 10), (2, 30), (5, 50), (9, 100)]
        calibration = build_calibration(anchors, "local_rational")
        for scan, bp in anchors:
            self.assertEqual(calibration.size(scan), bp)
        for scan, bp in anchors[1:-1]:
            self.assertAlmostEqual(calibration.size(scan - 1e-7), bp, delta=1e-5)
            self.assertAlmostEqual(calibration.size(scan + 1e-7), bp, delta=1e-5)
        self.assertIn("C0", calibration.diagnostics["continuity"])
        self.assertIn("C1 is not", calibration.diagnostics["continuity"])
        self.assertIsNone(calibration.size(-1e-8))
        self.assertIsNone(calibration.size(9 + 1e-8))
        self.assertIsNone(calibration.size(None))

    def test_near_zero_denominator_and_unresolved_middle_bp_rejected(self):
        pathological = [[(0, 1), (1, 1 + 1e-14), (2, 100)],
                        [(0, 1), (1, 100 - 1e-12), (2, 100)],
                        [(0, 1), (1, 1e6 - 1e-10), (2, 1e6)]]
        for anchors in pathological:
            with self.subTest(anchors=anchors), self.assertRaisesRegex(CalibrationError, "rational|unresolved"):
                build_calibration(anchors, "local_rational")

    def test_pathological_scans_and_nonfinite_input_rejected(self):
        for anchors in ([(0, 10), (1e-15, 30), (1, 50)],
                        [(0, 10), (0, 30), (1, 50)],
                        [(0, 10), (math.nan, 30), (1, 50)],
                        [(0, 10), (1, math.inf), (2, 50)]):
            with self.subTest(anchors=anchors), self.assertRaises(CalibrationError):
                build_calibration(anchors, "local_rational")

    def test_json_diagnostics_and_32_anchor_resource_bound(self):
        anchors = [(i * 10, 50 + i * 3) for i in range(32)]
        calibration = build_calibration(anchors, "local_rational")
        self.assertEqual(calibration.diagnostics["anchor_count"], 32)
        self.assertEqual(calibration.diagnostics["rational_triple_count"], 30)
        self.assertFalse(json.loads(json.dumps(calibration.diagnostics, allow_nan=False))["extrapolation"])
        with self.assertRaises(CalibrationError):
            build_calibration(anchors + [(320, 146)], "local_rational")


if __name__ == "__main__":
    unittest.main()
