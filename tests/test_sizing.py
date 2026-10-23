"""Independent sizing algorithm tests with generated, explicitly known markers."""
import itertools
import math
import random
import unittest

from genemapper_open.sizing import (MAX_CANDIDATES, MAX_EXPECTED,
                                   match_size_standard, parse_expected_bp)


EXPECTED = [75, 105, 155, 205, 275, 335, 425, 505, 605]


def nonlinear_scan(bp):
    return round(200 + 3.2 * bp + .0015 * bp * bp)


def reference_peaks():
    return [{"scan": nonlinear_scan(bp), "height": 90 + i * 5}
            for i, bp in enumerate(EXPECTED)]


class SizingTests(unittest.TestCase):
    def test_nonlinear_standard_with_taller_spurious_peaks(self):
        peaks = reference_peaks() + [{"scan": 900, "height": 90000}, {"scan": 2100, "height": 80000}]
        result = match_size_standard(peaks, EXPECTED)
        self.assertEqual(result["status"], "accepted", result["reasons"])
        self.assertEqual([row["scan"] for row in result["matched"]],
                         [nonlinear_scan(bp) for bp in EXPECTED])
        self.assertEqual([row["scan"] for row in result["unmatched"]], [900, 2100])
        self.assertLess(result["diagnostics"]["max_abs_error_bp"], 2.5)
        self.assertGreater(result["diagnostics"]["rms_error_bp"], 1)
        self.assertEqual(result["diagnostics"]["holdout_count"], 7)
        self.assertFalse(result["diagnostics"]["external_validation"])

    def test_calibration_has_no_extrapolation(self):
        result = match_size_standard(reference_peaks(), EXPECTED)
        calibration = result["calibration"]
        for bp in EXPECTED:
            self.assertAlmostEqual(calibration.size(nonlinear_scan(bp)), bp)
        self.assertIsNone(calibration.size(nonlinear_scan(EXPECTED[0]) - 1))
        self.assertIsNone(calibration.size(nonlinear_scan(EXPECTED[-1]) + 1))
        self.assertEqual(len(result["anchors"]), len(EXPECTED))

    def test_missing_marker_even_with_spurious_replacements_rejected(self):
        peaks = reference_peaks()
        del peaks[4]
        peaks.extend([{"scan": 900}, {"scan": 2100}])
        result = match_size_standard(peaks, EXPECTED)
        self.assertEqual(result["status"], "rejected")
        self.assertIn("missing markers", result["reasons"][0])
        self.assertIsNone(result["calibration"])
        self.assertEqual(result["anchors"], [])

    def test_insufficient_and_empty_candidates(self):
        for peaks in ([], reference_peaks()[:8]):
            result = match_size_standard(peaks, EXPECTED)
            self.assertEqual(result["status"], "rejected")
            self.assertIn("Fewer candidate", result["reasons"][0])

    def test_distorted_spacings_rejected(self):
        peaks = reference_peaks()
        peaks[4]["scan"] += 120
        result = match_size_standard(peaks, EXPECTED)
        self.assertEqual(result["status"], "rejected")
        self.assertIsNone(result["calibration"])

    def test_tight_residual_limit_rejects_nonlinearity(self):
        result = match_size_standard(reference_peaks(), EXPECTED, max_residual_bp=.1)
        self.assertEqual(result["status"], "rejected")

    def test_uniform_alternative_subsets_are_ambiguous(self):
        result = match_size_standard([{"scan": i * 100} for i in range(8)], [50, 100, 150, 200])
        self.assertEqual(result["status"], "rejected")
        self.assertIn("Ambiguous", result["reasons"][0])
        self.assertEqual(result["diagnostics"]["ambiguity_gap_bp"], 0)
        self.assertIsNone(result["calibration"])
        self.assertEqual(result["anchors"], [])

    def test_alternative_interior_peak_with_same_endpoints_is_not_hidden(self):
        bp = [50, 85, 145, 225, 310]
        peaks = [{"scan": size * 4} for size in bp] + [{"scan": 145 * 4 + 1}]
        result = match_size_standard(peaks, bp)
        self.assertEqual(result["status"], "rejected")
        self.assertIn("Ambiguous", result["reasons"][0])
        self.assertLess(result["diagnostics"]["ambiguity_gap_bp"], .5)
        self.assertEqual(result["matched"][0]["scan"], result["diagnostics"]["alternative_match"][0]["scan"])
        self.assertEqual(result["matched"][-1]["scan"], result["diagnostics"]["alternative_match"][-1]["scan"])

    def test_zero_ambiguity_margin_still_rejects_exact_ties(self):
        result = match_size_standard([{"scan": i * 100} for i in range(8)],
                                     [50, 100, 150, 200], ambiguity_margin_bp=0)
        self.assertEqual(result["status"], "rejected")

    def test_input_order_and_height_do_not_change_matching(self):
        peaks = reference_peaks() + [{"scan": 900, "height": 100000}]
        first = match_size_standard(peaks, EXPECTED)
        random.Random(7).shuffle(peaks)
        for peak in peaks:
            peak["height"] = 1
        second = match_size_standard(peaks, EXPECTED)
        self.assertEqual(first["anchors"], second["anchors"])
        self.assertEqual(first["status"], second["status"])

    def test_noise_candidates_do_not_match(self):
        rng = random.Random(917)
        peaks = [{"scan": scan} for scan in sorted(rng.sample(range(50, 3000), 16))]
        result = match_size_standard(peaks, EXPECTED)
        self.assertEqual(result["status"], "rejected")

    def test_four_markers_with_two_holdouts(self):
        bp = [50, 95, 160, 270]
        result = match_size_standard([{"scan": 20 + size * 3} for size in bp], bp)
        self.assertEqual(result["status"], "accepted")
        self.assertEqual(result["diagnostics"]["holdout_residuals_bp"], [None, 0, 0, None])
        self.assertEqual(result["diagnostics"]["holdout_count"], 2)

    def test_invalid_settings(self):
        for name, values in (("max_residual_bp", [0, -1, math.nan, math.inf, 101, True, "5"]),
                             ("ambiguity_margin_bp", [-1, math.nan, math.inf, 101, False, "0.5"])):
            for value in values:
                with self.subTest(name=name, value=value):
                    result = match_size_standard(reference_peaks(), EXPECTED, **{name: value})
                    self.assertEqual(result["status"], "rejected")
                    self.assertIn(name, result["reasons"][0])

    def test_invalid_fragment_sizes(self):
        for values in ([50, 75, 75, 100], [100, 75, 50, 25], [1, 2, 3],
                       [0, 50, 75, 100], [10, 20, math.nan, 100], [10, 20, math.inf, 100],
                       [10, 20, 30, 1000001], [10, 20, 30, True], [10, 20, 30, "50"],
                       [1, 2, 3, 10 ** 1000]):
            with self.subTest(values=values):
                result = match_size_standard(reference_peaks(), values)
                self.assertEqual(result["status"], "rejected")
                self.assertIsNone(result["calibration"])

    def test_invalid_candidates(self):
        base = reference_peaks()
        for invalid in ({}, {"scan": -1}, {"scan": math.nan}, {"scan": math.inf},
                        {"scan": True}, {"scan": "40"}, {"scan": 1e9},
                        {"scan": 1, "height": math.nan}, {"scan": 1, "height": math.inf},
                        {"scan": 1, "height": "high"}, None, 17):
            with self.subTest(invalid=invalid):
                result = match_size_standard(base + [invalid], EXPECTED)
                self.assertEqual(result["status"], "rejected")
                self.assertIn("Candidate", result["reasons"][0])

    def test_duplicate_scans_rejected(self):
        result = match_size_standard(reference_peaks() + [reference_peaks()[0]], EXPECTED)
        self.assertEqual(result["status"], "rejected")
        self.assertIn("distinct", result["reasons"][0])

    def test_unbounded_iterables_rejected_without_iteration(self):
        result = match_size_standard(itertools.repeat({"scan": 1}), EXPECTED)
        self.assertEqual(result["status"], "rejected")
        result = match_size_standard(reference_peaks(), itertools.count())
        self.assertEqual(result["status"], "rejected")

    def test_resource_limits_before_matching(self):
        result = match_size_standard([{"scan": i} for i in range(MAX_CANDIDATES + 1)], EXPECTED)
        self.assertEqual(result["status"], "rejected")
        self.assertIn("select a scan interval", result["reasons"][0])
        self.assertEqual(result["diagnostics"]["transitions_checked"], 0)
        result = match_size_standard(reference_peaks(), list(range(1, MAX_EXPECTED + 2)))
        self.assertEqual(result["status"], "rejected")
        self.assertEqual(result["diagnostics"]["transitions_checked"], 0)

    def test_bounded_work_at_maximum_input(self):
        result = match_size_standard([{"scan": i * 10} for i in range(MAX_CANDIDATES)],
                                     list(range(10, 10 * MAX_EXPECTED + 1, 10)),
                                     max_residual_bp=100)
        self.assertEqual(result["status"], "rejected")  # many equally spaced alternatives
        self.assertLessEqual(result["diagnostics"]["transitions_checked"],
                             2 * MAX_EXPECTED * math.comb(MAX_CANDIDATES, 3))

    def test_dynamic_program_agrees_with_small_exhaustive_oracle(self):
        """An independent exhaustive subset oracle checks best and runner-up."""
        rng = random.Random(1024)
        expected = [50, 95, 175, 260]
        for _ in range(12):
            scans = sorted(rng.sample(range(50, 1500), 8))
            possibilities = []
            for indexes in itertools.combinations(range(8), 4):
                errors = []
                for k in (1, 2):
                    a, b, c = (scans[indexes[j]] for j in (k - 1, k, k + 1))
                    predicted = expected[k - 1] + (b - a) * (expected[k + 1] - expected[k - 1]) / (c - a)
                    errors.append(predicted - expected[k])
                if all(abs(error) <= 100 for error in errors):
                    possibilities.append((math.sqrt(sum(error * error for error in errors) / 2), indexes))
            possibilities.sort()
            result = match_size_standard([{"scan": scan} for scan in scans], expected,
                                         max_residual_bp=100, ambiguity_margin_bp=0)
            self.assertTrue(possibilities)
            self.assertAlmostEqual(result["diagnostics"]["rms_error_bp"], possibilities[0][0])
            self.assertEqual([row["scan"] for row in result["matched"]],
                             [scans[i] for i in possibilities[0][1]])
            if len(possibilities) > 1:
                self.assertAlmostEqual(result["diagnostics"]["second_best_rms_error_bp"], possibilities[1][0])


class ParseFragmentTests(unittest.TestCase):
    def test_mixed_separators_and_decimal_sizes(self):
        self.assertEqual(parse_expected_bp("75, 105; 155.5\n205\t275"), [75, 105, 155.5, 205, 275])

    def test_invalid_input(self):
        for text in ("", "75,105,155", "75,75,155,205", "105,75,155,205", "0,75,155,205",
                     "-1,75,155,205", "nan,75,155,205", "75,105,155,inf", "75,105,155,kit",
                     "1,2,3,1000001", None, 15):
            with self.subTest(text=text), self.assertRaises(ValueError):
                parse_expected_bp(text)

    def test_size_limits_before_unbounded_splitting(self):
        for text in ("1," * 3000, ",".join(map(str, range(1, 34)))):
            with self.assertRaises(ValueError):
                parse_expected_bp(text)
        self.assertEqual(len(parse_expected_bp(" ".join(map(str, range(1, 33))))), 32)


if __name__ == "__main__":
    unittest.main()
