"""Integration checks use independent synthetic migration ground truth."""
import hashlib
import json
import math
import struct
import unittest

from genemapper_open.abif import AbifFile
from genemapper_open.demo import SYNTHETIC_STANDARD_BP, sizing_demo_bytes, build_abif
from genemapper_open.server import analyze


class SizingWorkflowTests(unittest.TestCase):
    def setUp(self):
        self.raw = sizing_demo_bytes()
        self.options = dict(sizing_mode="auto", standard_channel="DATA106",
                            standard_bp=list(SYNTHETIC_STANDARD_BP))

    def test_synthetic_alignment_and_sized_peaks(self):
        result = analyze(self.raw, "synthetic.fsa", **self.options)
        report = result["sizing"]
        self.assertEqual(report["status"], "accepted", report)
        self.assertEqual(len(report["matched"]), 9)
        self.assertEqual(len(report["unmatched"]), 2)
        for match, bp in zip(report["matched"], SYNTHETIC_STANDARD_BP):
            expected_scan = 200 + 3.2 * bp + .0015 * bp ** 2
            self.assertAlmostEqual(match["scan"], expected_scan, delta=1)
        for channel in result["channels"]:
            for peak in channel["peaks"]:
                scan = peak["scan"]
                true_bp = (-3.2 + math.sqrt(3.2 ** 2 + .006 * (scan - 200))) / .003
                if report["anchors"][0][0] <= scan <= report["anchors"][-1][0]:
                    self.assertAlmostEqual(peak["size_bp"], true_bp, delta=1.5)
                else:
                    self.assertIsNone(peak["size_bp"])
        self.assertEqual(result["source_sha256"], hashlib.sha256(self.raw).hexdigest())
        json.dumps(result, allow_nan=False)

    def test_missing_marker_withholds_every_size(self):
        result = analyze(self.raw, "synthetic.fsa", standard_end=2400, **self.options)
        self.assertEqual(result["sizing"]["status"], "rejected")
        self.assertEqual(result["calibration"], [])
        self.assertTrue(all("size_bp" not in p for c in result["channels"] for p in c["peaks"]))

    def test_no_standard_is_assumed(self):
        result = analyze(self.raw, "synthetic.fsa")
        self.assertEqual(result["sizing"]["status"], "not_requested")
        self.assertEqual(result["calibration"], [])

    def test_invalid_reference_and_settings(self):
        for options in (dict(standard_channel="missing"), dict(standard_start=-1),
                        dict(standard_end=3000), dict(max_residual_bp=float("nan")),
                        dict(standard_bp="75,75,150,200")):
            with self.subTest(options=options), self.assertRaises(ValueError):
                analyze(self.raw, "synthetic.fsa", **(self.options | options))

    def test_manual_and_automatic_cannot_mix(self):
        with self.assertRaises(ValueError):
            analyze(self.raw, "synthetic.fsa", anchors="100:75,1500:400", **self.options)
        with self.assertRaises(ValueError):
            analyze(self.raw, "synthetic.fsa", sizing_mode="manual")

    def test_all_calibration_models_are_explicit_and_serializable(self):
        for model in ("linear", "lsq2", "lsq3", "natural_cubic", "local_rational"):
            with self.subTest(model=model):
                result = analyze(self.raw, "synthetic.fsa", calibration_model=model, **self.options)
                self.assertEqual(result["sizing"]["status"], "accepted", result["sizing"])
                self.assertEqual(result["sizing"]["calibration_model"], model)
                self.assertTrue(result["sizing"]["calibration_diagnostics"]["monotonic"])
                self.assertLessEqual(result["sizing"]["calibration_diagnostics"]["max_abs_anchor_error_bp"], 5)
                json.dumps(result, allow_nan=False)

    def test_nonmonotonic_curve_withholds_sizes_after_successful_match(self):
        values = [0] * 500
        for scan in (100, 200, 300, 400):
            values[scan] = 500
        raw = build_abif([(b"DATA", 1, 4, 2, len(values), struct.pack(">500h", *values))])
        result = analyze(raw, "synthetic.fsa", sizing_mode="auto", standard_channel="DATA1",
                         standard_bp=[1, 2, 2.01, 3], calibration_model="lsq3")
        self.assertEqual(len(result["sizing"]["matched"]), 4)
        self.assertEqual(result["sizing"]["status"], "rejected")
        self.assertIn("Calibration model rejected", result["sizing"]["reasons"][0])
        self.assertEqual(result["calibration"], [])
        self.assertTrue(all("size_bp" not in p for c in result["channels"] for p in c["peaks"]))

    def test_unknown_curve_and_insufficient_manual_anchors(self):
        with self.assertRaises(ValueError):
            analyze(self.raw, "synthetic.fsa", calibration_model="vendor_magic")
        with self.assertRaises(ValueError):
            analyze(self.raw, "synthetic.fsa", anchors="100:75,2000:400", calibration_model="lsq3")


if __name__ == "__main__":
    unittest.main()
