import csv
import io
import math
from pathlib import Path
import struct
import unittest

from genemapper_open.abif import AbifError, AbifFile, ENTRY
from genemapper_open.analysis import Calibration, find_peaks, parse_anchors
from genemapper_open.demo import build_abif, demo_bytes
from genemapper_open.export import peaks_csv, traces_csv
from genemapper_open.server import analyze


def one_tag(name=b"TEST", kind=4, width=2, count=2, payload=b"\xff\xfe\x00\x07"):
    return build_abif([(name, 1, kind, width, count, payload)])


class ParserTests(unittest.TestCase):
    def test_inline_signed_big_endian(self):
        file = AbifFile(one_tag())
        self.assertEqual(file.value("TEST"), [-2, 7])
        self.assertEqual(file.directory()[0]["inline"], True)

    def test_external_numeric_big_endian(self):
        file = AbifFile(one_tag(kind=5, width=4, count=3,
                               payload=struct.pack(">3i", -100000, 0, 123456)))
        self.assertEqual(file.value("TEST"), [-100000, 0, 123456])

    def test_unsigned_word_and_float(self):
        file = AbifFile(build_abif([(b"WORD", 1, 3, 2, 2, struct.pack(">HH", 65535, 1)),
                                   (b"FLOT", 1, 7, 4, 2, struct.pack(">ff", 1.5, -2.25))]))
        self.assertEqual(file.value("WORD"), [65535, 1])
        self.assertEqual(file.value("FLOT"), [1.5, -2.25])

    def test_pascal_c_and_char_strings(self):
        file = AbifFile(build_abif([(b"PSTR", 1, 18, 1, 3, b"\x02AB"),
                                   (b"CSTR", 1, 19, 1, 5, b"ABCD\x00"),
                                   (b"CHAR", 1, 2, 1, 4, b"GATC")]))
        self.assertEqual(file.value("PSTR"), "AB")
        self.assertEqual(file.value("CSTR"), "ABCD")
        self.assertEqual(file.value("CHAR"), "GATC")

    def test_unknown_user_type_stays_opaque(self):
        file = AbifFile(one_tag(kind=1024, width=33, count=400, payload=b"hello"))
        self.assertEqual(file.value("TEST"), b"hello")

    def test_date_time(self):
        file = AbifFile(build_abif([(b"DATE", 1, 10, 4, 1, struct.pack(">HBB", 2026, 10, 19)),
                                   (b"TIME", 1, 11, 4, 1, b"\x0c\x22\x38\x00")]))
        self.assertEqual(file.value("DATE"), [2026, 10, 19])
        self.assertEqual(file.value("TIME"), [12, 34, 56, 0])

    def test_root_reserved_fields_ignored(self):
        raw = bytearray(one_tag())
        struct.pack_into(">H", raw, 14, 7)
        struct.pack_into(">I", raw, 22, 999999)
        self.assertEqual(AbifFile(raw).value("TEST"), [-2, 7])

    def test_bad_magic_and_short_header(self):
        for data in (b"", b"ABIF", b"FAIL" + b"\0" * 124):
            with self.subTest(data=data[:4]), self.assertRaises(AbifError):
                AbifFile(data)

    def test_hid_v3_rejected_explicitly(self):
        raw = bytearray(one_tag())
        struct.pack_into(">H", raw, 4, 300)
        with self.assertRaisesRegex(AbifError, "Unsupported ABIF version 300"):
            AbifFile(raw)

    def test_directory_bounds(self):
        for offset in (0, 127, 999999, 0xffffffff):
            raw = bytearray(one_tag())
            struct.pack_into(">I", raw, 26, offset)
            with self.subTest(offset=offset), self.assertRaises(AbifError):
                AbifFile(raw)

    def test_huge_directory_count(self):
        raw = bytearray(one_tag())
        struct.pack_into(">I", raw, 18, 0xffffffff)
        with self.assertRaises(AbifError):
            AbifFile(raw)

    def test_truncated_external_payload(self):
        raw = one_tag(count=4, payload=struct.pack(">4h", 1, 2, 3, 4))
        with self.assertRaisesRegex(AbifError, "outside file"):
            AbifFile(raw[:-1])

    def test_wrong_numeric_count(self):
        with self.assertRaisesRegex(AbifError, "count/size"):
            AbifFile(one_tag(count=3))

    def test_wrong_numeric_width(self):
        with self.assertRaisesRegex(AbifError, "count/size"):
            AbifFile(one_tag(width=1))

    def test_wrong_string_count_and_width(self):
        for kind in (2, 18, 19):
            for count, width in ((4, 1), (3, 2)):
                with self.subTest(kind=kind, count=count, width=width), self.assertRaises(AbifError):
                    AbifFile(one_tag(kind=kind, count=count, width=width, payload=b"\x01A\x00"))

    def test_truncated_pascal(self):
        with self.assertRaisesRegex(AbifError, "Pascal"):
            AbifFile(one_tag(kind=18, width=1, count=4, payload=b"\x04abc"))

    def test_unterminated_c_string(self):
        with self.assertRaisesRegex(AbifError, "unterminated"):
            AbifFile(one_tag(kind=19, width=1, count=4, payload=b"abcd"))

    def test_duplicate_tags(self):
        row = (b"TEST", 1, 1, 1, 1, b"\x01")
        with self.assertRaisesRegex(AbifError, "Duplicate"):
            AbifFile(build_abif([row, row]))

    def test_invalid_tag_name(self):
        with self.assertRaisesRegex(AbifError, "ASCII"):
            AbifFile(one_tag(name=b"A\x00BC"))

    def test_generated_six_channel_data(self):
        sample = AbifFile(demo_bytes())
        self.assertEqual(sample.value("SMPL"), "Synthetic demo")
        channels = sample.channels()
        self.assertEqual([c["number"] for c in channels], [1, 2, 3, 4, 105, 106])
        self.assertEqual([c["dye_index"] for c in channels], [1, 2, 3, 4, 5, 6])
        self.assertTrue(all(len(c["values"]) == 3000 for c in channels))
        self.assertEqual(channels[4]["dye"], "Dye 5")

    def test_auxiliary_is_not_a_dye(self):
        sample = AbifFile(build_abif([(b"DATA", 5, 4, 2, 3, struct.pack(">3h", 1, 2, 3))]))
        channel = sample.channels()[0]
        self.assertEqual(channel["group"], "auxiliary")
        self.assertIsNone(channel["dye_index"])


class AnalysisTests(unittest.TestCase):
    def test_known_peak_locations(self):
        signal = [0] * 301
        signal[50:55] = [5, 30, 80, 30, 5]
        signal[150:155] = [8, 40, 100, 40, 8]
        peaks = find_peaks(signal, threshold=50)
        self.assertEqual([p["scan"] for p in peaks], [52, 152])
        self.assertEqual(peaks[0]["height"], 80)
        self.assertEqual(peaks[0]["area"], 150)

    def test_plateau_and_distance(self):
        signal = [0] * 101
        signal[10:13] = [50] * 3
        signal[15] = 100
        self.assertEqual([p["scan"] for p in find_peaks(signal, 20, 8)], [15])
        self.assertEqual([p["scan"] for p in find_peaks(signal, 20, 2)], [11, 15])

    def test_flat_and_empty(self):
        self.assertEqual(find_peaks([], 0), [])
        self.assertEqual(find_peaks([100] * 200, 0), [])

    def test_invalid_parameters(self):
        for threshold in (-1, math.inf, math.nan):
            with self.assertRaises(ValueError):
                find_peaks([0, 10, 0], threshold)
        with self.assertRaises(ValueError):
            find_peaks([0, math.nan, 0])
        with self.assertRaises(ValueError):
            find_peaks([0, 10, 0], min_distance=0)

    def test_calibration_interpolation_and_no_extrapolation(self):
        calibration = Calibration([(100, 50), (300, 150), (500, 350)])
        self.assertEqual(calibration.size(200), 100)
        self.assertEqual(calibration.size(400), 250)
        self.assertEqual(calibration.size(100), 50)
        self.assertEqual(calibration.size(500), 350)
        self.assertIsNone(calibration.size(99))
        self.assertIsNone(calibration.size(501))

    def test_bad_anchors(self):
        for anchors in ([], [(1, 2)], [(1, 2), (1, 3)], [(1, 3), (2, 2)],
                        [(0, math.nan), (1, 2)], [(-1, 2), (2, 3)]):
            with self.subTest(anchors=anchors), self.assertRaises(ValueError):
                Calibration(anchors)
        self.assertIsNone(parse_anchors(""))
        self.assertEqual(parse_anchors("100:50\n300:150").size(200), 100)

    def test_integrated_demo_peak_recovery(self):
        result = analyze(demo_bytes(), "demo.fsa", anchors="0:1,2999:600")
        self.assertEqual(len(result["channels"]), 6)
        for channel in result["channels"]:
            self.assertEqual(len(channel["peaks"]), 4)
            self.assertTrue(all(p["size_bp"] > 1 for p in channel["peaks"]))

    def test_csv_signal_roundtrip(self):
        channels = AbifFile(demo_bytes()).channels()
        rows = list(csv.reader(io.StringIO(traces_csv(channels))))
        self.assertEqual(rows[0], ["scan", "DATA1", "DATA2", "DATA3", "DATA4", "DATA105", "DATA106"])
        self.assertEqual(len(rows), 3001)
        self.assertEqual(int(rows[100][1]), channels[0]["values"][99])

    def test_csv_peak_export(self):
        result = analyze(demo_bytes(), "demo.fsa")
        rows = list(csv.reader(io.StringIO(peaks_csv(result["channels"]))))
        self.assertEqual(len(rows), 25)
        self.assertEqual(rows[0][-1], "size_bp")

    def test_no_numeric_data_gives_explicit_error(self):
        raw = build_abif([(b"DATA", 1, 28, 1, 5, b"hello")])
        self.assertEqual(len(AbifFile(raw).summary()["warnings"]), 1)
        with self.assertRaisesRegex(AbifError, "No supported numeric DATA"):
            analyze(raw, "opaque.fsa")

    def test_opaque_metadata_stays_out_of_json(self):
        raw = build_abif([(b"DATA", 1, 4, 2, 3, struct.pack(">3h", 0, 50, 0)),
                          (b"SMPL", 1, 1024, 1, 5, b"hello")])
        self.assertEqual(analyze(raw, "opaque.fsa")["sample"], "")


if __name__ == "__main__":
    unittest.main()
