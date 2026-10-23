import copy
import math
import unittest

from genemapper_open.standard_wrapper import wrap_standard_peaks
from genemapper_open.sizing_pipeline import analyze_size_standard
from genemapper_open.standard_prefilters import filter_standard_peaks


def peak(scan=15.4, begin=13.2, end=17.8, **fields):
    return dict(scan=scan, begin=begin, end=end, height=100.0, begin_height=12.5,
                end_height=13.5, area=451, fwhm_scan=2.7, **fields)


class StandardWrapperTests(unittest.TestCase):
    def test_cast_then_offset_retains_measured_features_and_input(self):
        original = [peak()]
        snapshot = copy.deepcopy(original)
        result = wrap_standard_peaks(original, signal_length=31, offset=100)
        actual = result['peaks'][0]
        self.assertEqual((actual['begin'], actual['scan'], actual['end']), (113,115,117))
        for key in ('height', 'begin_height', 'end_height', 'area', 'fwhm_scan'):
            self.assertEqual(actual[key], original[0][key])
        self.assertEqual(actual['wrapper_source_local'], dict(begin=13.2,scan=15.4,end=17.8))
        self.assertEqual(original, snapshot)
        self.assertEqual(result['diagnostics']['fractional_coordinates_truncated'], 3)

    def test_offscale_uses_integer_absolute_bounds_and_inclusive_touch(self):
        source = [peak()]
        touching = wrap_standard_peaks(source, signal_length=31, offset=100, offscale_intervals=[(113,113)])
        outside = wrap_standard_peaks(source, signal_length=31, offset=100, offscale_intervals=[(117.5,200)])
        self.assertTrue(touching['peaks'][0]['offscaled'])
        self.assertFalse(outside['peaks'][0]['offscaled'])
        retained = wrap_standard_peaks([peak(offscaled=True)], signal_length=31)
        self.assertTrue(retained['peaks'][0]['offscaled'])

    def test_sort_and_duplicate_projection_are_visible(self):
        result = wrap_standard_peaks([peak(16.1),peak(15.7),peak(15.2)], signal_length=31)
        self.assertEqual([p['scan'] for p in result['peaks']], [15,15,16])
        self.assertEqual([p['wrapper_source_local']['scan'] for p in result['peaks']], [15.7,15.2,16.1])
        self.assertEqual(result['diagnostics']['duplicate_projected_scans'], 1)

    def test_nonzero_offset_is_not_used_as_a_local_signal_index(self):
        projected = wrap_standard_peaks([peak()], signal_length=31, offset=1000)
        self.assertEqual(projected['diagnostics']['output_coordinate_scope'], 'absolute')
        with self.assertRaises(ValueError):
            wrap_standard_peaks(projected['peaks'], signal_length=31)

    def test_input_limits_and_double_projection(self):
        for kwargs in ({'signal_length': True}, {'signal_length': 0},
                       {'signal_length': 31, 'offset': -1}, {'signal_length': 31, 'offset': 2**31},
                       {'signal_length': 31, 'offscale_intervals': [(float('nan'),20)]}):
            with self.assertRaises(ValueError):
                wrap_standard_peaks([peak()], **kwargs)
        with self.assertRaises(ValueError):
            wrap_standard_peaks([peak(begin=-0.1)], signal_length=31)
        first = wrap_standard_peaks([peak()], signal_length=31)['peaks']
        with self.assertRaises(ValueError):
            wrap_standard_peaks(first, signal_length=31)

    def test_pipeline_wrapper_preserves_known_synthetic_assignments(self):
        expected = [40,70,110,145,195,260]
        centers = [200+20*bp for bp in expected]
        raw = [round(20+sum(1000*math.exp(-0.5*((i-center)/5)**2) for center in centers)) for i in range(6501)]
        fractional = analyze_size_standard(raw, expected, num_extra_peaks=25, quality_threshold=0.75)
        wrapped = analyze_size_standard(raw, expected, num_extra_peaks=25, quality_threshold=0.75,
                                       coordinate_mode='standard_wrapper')
        self.assertEqual(wrapped['status'], 'accepted')
        self.assertEqual(wrapped['anchors'], fractional['anchors'])
        self.assertEqual(wrapped['coordinate_conversion']['offset'], 0)
        self.assertTrue(all(type(p['begin']) is int and type(p['end']) is int for p in wrapped['peaks']))
        invalid = analyze_size_standard(raw, expected, num_extra_peaks=25, quality_threshold=0.75, coordinate_mode='guess')
        self.assertEqual(invalid['status'], 'rejected')
        self.assertEqual(invalid['stage'], 'validation')

    def test_connected_tail_checks_one_more_sample_after_projection(self):
        def feature(scan, height=100, beginning_height=1):
            return {'scan': scan, 'begin': scan-5, 'end': scan+5,
                    'height': height, 'begin_height': beginning_height,
                    'end_height': 1, 'fwhm_scan': 3}
        artifact = feature(50, 20000)
        artifact['end'] = 100.8
        first = feature(150, beginning_height=40)
        first['begin'] = 140.2
        candidates = [feature(10), artifact, first]+[feature(i,1000) for i in (250,350,450)]
        signal = [40]*1000
        signal[140] = 39
        fractional = filter_standard_peaks(candidates, 3, signal)
        converted = wrap_standard_peaks(candidates, signal_length=len(signal))['peaks']
        integer = filter_standard_peaks(converted, 3, signal)
        # Fractional int(140.2-100.8+1)=40 copies100..139.
        # Integer bounds copy100..140, observing the low sample at140.
        self.assertEqual(fractional['diagnostics']['connected_tail_removed_indices'], [2])
        self.assertEqual(integer['diagnostics']['connected_tail_removed_indices'], [])


if __name__ == '__main__':
    unittest.main()
