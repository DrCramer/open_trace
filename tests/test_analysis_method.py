import copy
import json
from pathlib import Path
import sys
import unittest

from genemapper_open.analysis_method import (FACTORY, analyze_settings, factory_method, kit_marker_parameters,
    ladder_quality_definition, load_analysis_method, marker_quality_definition, method_request, parse_gm_xml,
    pullup_definition, to_gm_xml, unsupported)
from genemapper_open.gm_kit_import import import_kit
sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_gm_kit_import import BINS, PANELS, STUTTER  # noqa: E402

GS600 = [60, 80, 100, 114, 120, 140, 160, 180, 200, 214, 220, 240, 250, 260, 280, 300, 314, 320, 340, 360,
         380, 400, 414, 420, 440, 460]

# Minimal original-style container (element names as written by the original).
XML = """<?xml version="1.0" encoding="UTF-8"?>
<AnalysisMethodContainer xmlns="www.AppliedBiosystems.com"><appVersion>1.0</appVersion><xmlAnalysisMethod>
<analysisMethodName>Lab GF 175</analysisMethodName><applicationMode>2</applicationMode>
<analysisFullRange>false</analysisFullRange><analysisStartPt>1200</analysisStartPt><analysisStopPt>9000</analysisStopPt>
<sizeCallAllSizes>true</sizeCallAllSizes><smoothValue>8</smoothValue>
<thresholdMap><Key>Genotype Quality (GQ)</Key><QualityThreshold><goodIndicator>0.8</goodIndicator><badIndicator>0.2</badIndicator></QualityThreshold></thresholdMap>
<baselinerParameter><windowSize>101</windowSize></baselinerParameter>
<peakDetectorParameter><degree>3</degree><window>15</window><windowTetri>9</windowTetri><minHeightBlue>175</minHeightBlue>
<minHeightGreen>175</minHeightGreen><minHeightYellow>175</minHeightYellow><minHeightRed>175</minHeightRed>
<minHeightOrange>175</minHeightOrange><minHeightPurple>175</minHeightPurple><minWidth>2</minWidth><tauB>0.0</tauB><tauE>0.0</tauE></peakDetectorParameter>
<sizeCallerParameter><sizeCallMethod>Least Square Third Order</sizeCallMethod></sizeCallerParameter>
<alleleCallerParameter><tetriNucleotide_cutOffValue>0.1</tetriNucleotide_cutOffValue>
<tetriNucleotide_minusARatio>0.3</tetriNucleotide_minusARatio><tetriNucleotide_minusADistance>1.6</tetriNucleotide_minusADistance>
<tetriNucleotide_minusADistanceRight>0.0</tetriNucleotide_minusADistanceRight>
<tetriNucleotide_stutterRatio>0.15</tetriNucleotide_stutterRatio><tetriNucleotide_stutterDistance>4.75</tetriNucleotide_stutterDistance>
<tetriNucleotide_stutterDistanceRight>3.25</tetriNucleotide_stutterDistanceRight>
<useMarkerStutterPercentage>true</useMarkerStutterPercentage><binSet>GlobalFiler_Bins_v1</binSet>
<warningPeakHeightRatioThreshold>0.6</warningPeakHeightRatioThreshold><warningMaxAlleleNumber>3</warningMaxAlleleNumber>
<clearRangeBlueDyeFrom>70.0</clearRangeBlueDyeFrom><clearRangeBlueDyeTo>75.0</clearRangeBlueDyeTo>
<spikeDetectionEnabled>false</spikeDetectionEnabled></alleleCallerParameter>
<qualityValueProcessParameter><binningQualityWeight>0.5</binningQualityWeight></qualityValueProcessParameter>
</xmlAnalysisMethod></AnalysisMethodContainer>"""


class MethodTests(unittest.TestCase):
    def test_factory_values_from_original_factory_defaults(self):
        m = factory_method()
        self.assertEqual(m['peak_detector']['thresholds'], dict.fromkeys(m['peak_detector']['thresholds'], 50))
        self.assertEqual((m['peak_detector']['smoothing'], m['peak_detector']['baseline_window']), ('light', 51))
        self.assertEqual(m['peak_detector']['size_calling'], 'local_southern')
        tetra = m['allele']['repeats']['4']
        self.assertEqual((tetra['minus_stutter_from'], tetra['minus_stutter_to'], tetra['cutoff']), (3.25, 4.75, 0.0))
        q = m['peak_quality']
        self.assertEqual((q['lph_homozygous'], q['lph_heterozygous'], q['mph'], q['phr'], q['bd_bp']), (200, 100, 5000, 0.7, 1.5))
        self.assertEqual(m['sq_gq']['weights']['acc'], 0.0)
        self.assertEqual(unsupported(m), [])

    def test_validation_errors(self):
        bad = []
        for path, value in (('name', ''), ('peak_detector.peak_window', 14), ('peak_detector.polynomial_degree', 20),
                            ('peak_detector.smoothing', 'medium'), ('allele.repeats.4.cutoff', 1.5),
                            ('sq_gq.gq.low', 0.9), ('peak_detector.slope_end', 1.0), ('peak_detector.baseline_window', 50)):
            m = copy.deepcopy(FACTORY); node = m; keys = path.split('.')
            for k in keys[:-1]:
                node = node[k]
            node[keys[-1]] = value
            with self.subTest(path=path), self.assertRaises(ValueError):
                load_analysis_method(m)
        m = copy.deepcopy(FACTORY); m['extra'] = 1
        with self.assertRaises(ValueError):
            load_analysis_method(m)

    def test_xml_import_export_round_trip(self):
        m = parse_gm_xml(XML)
        self.assertEqual(m['name'], 'Lab GF 175')
        self.assertEqual(m['peak_detector']['analysis_range'], {'full': False, 'start': 1200, 'stop': 9000})
        self.assertEqual((m['peak_detector']['smoothing'], m['peak_detector']['baseline_window']), ('heavy', 101))
        self.assertEqual(m['peak_detector']['size_calling'], 'lsq3')
        self.assertEqual(m['peak_detector']['thresholds']['purple'], 175)
        t = m['allele']['repeats']['4']
        self.assertEqual((t['cutoff'], t['minus_a_ratio'], t['minus_a_from'], t['minus_a_to']), (0.1, 0.3, 0.0, 1.6))
        self.assertEqual(m['allele']['range_filter']['blue'], {'from': 70.0, 'to': 75.0})
        self.assertFalse(m['peak_quality']['ladder_spike']['enabled'])
        self.assertEqual((m['peak_quality']['phr'], m['peak_quality']['max_alleles']), (0.6, 3))
        self.assertEqual(m['sq_gq']['gq'], {'pass': 0.8, 'low': 0.2})
        self.assertEqual(m['sq_gq']['weights']['bin'], 0.5)
        self.assertEqual(parse_gm_xml(to_gm_xml(m)), m)
        self.assertEqual(parse_gm_xml(to_gm_xml(factory_method())), factory_method())

    def test_xml_rejections(self):
        for text in ('<x/>', 'not xml', '<!DOCTYPE a [<!ENTITY e "x">]><AnalysisMethodContainer/>',
                     XML.replace('<applicationMode>2', '<applicationMode>1'),
                     XML.replace('Least Square Third Order', 'Magic'), XML.replace('<smoothValue>8', '<smoothValue>5')):
            with self.subTest(text=text[:30]), self.assertRaises(ValueError):
                parse_gm_xml(text)

    def test_analyze_settings(self):
        m = parse_gm_xml(XML)
        s = analyze_settings(m, GS600)
        self.assertEqual(s['calibration_model'], 'lsq3')
        self.assertEqual(s['analysis_range'], [1200, 9000])
        self.assertEqual(s['channel_thresholds']['DATA106'], 175.0)
        self.assertEqual(s['detector'], {'baseline_window': 101, 'smooth_passes': 8, 'window': 9, 'standard_window': 15,
                                         'min_width': 2.0, 'tau_begin': 0.0, 'tau_end': 0.0})
        self.assertEqual(s['spike_definition']['adjustments']['ladder_height_ratio'], 1e12)  # ladder spikes disabled
        pairs = s['pullup_definition']['detector']['channel_pairs']
        self.assertTrue(all(p['candidate_channel'] != 'DATA105' for p in pairs))
        m['peak_detector']['sizing_range'] = {'all': False, 'start_bp': 100, 'stop_bp': 400}
        self.assertTrue(analyze_settings(m, GS600)['standard_bp'].startswith('100,114'))
        m['peak_detector']['size_calling'] = 'global_southern'
        with self.assertRaises(ValueError):
            analyze_settings(m, GS600)

    def test_kit_and_quality_definitions(self):
        m = parse_gm_xml(XML)
        params = kit_marker_parameters(m, 4, 'blue')
        self.assertEqual(params['clear_ranges'], [{'min_bp': 70.0, 'max_bp': 75.0}])
        self.assertEqual(kit_marker_parameters(m, 9, 'green')['cutoff_ratio'], 0.0)
        kit = import_kit(PANELS, BINS, STUTTER, analysis_method=m)
        m1 = kit['markers'][0]
        self.assertEqual((m1['parameters']['cutoff_ratio'], m1['parameters']['minus_a_ratio']), (0.1, 0.3))
        self.assertEqual(m1['parameters']['ladder_cutoff_ratio'], 0.03)
        # M2 has no marker stutter: the method's global tetra stutter applies.
        self.assertEqual(kit['markers'][1]['parameters']['backward_stutter_ranges'],
                         [{'min_bp': 3.25, 'max_bp': 4.75, 'ratio': 0.15}])
        mq = marker_quality_definition(m, kit)
        self.assertEqual(mq['markers']['Y1']['kind'], 'y')
        self.assertEqual(mq['markers']['M1']['thresholds']['max_alleles'], 3)
        self.assertEqual(mq['gq_thresholds'], {'good': 0.8, 'bad': 0.2})
        lq = ladder_quality_definition(m, kit)
        self.assertEqual(lq['markers']['M1']['spike_weight'], 0.0)
        with self.assertRaises(ValueError):
            import_kit(PANELS, BINS, analysis_method=m, method={'cutoff_ratio': 0.1})

    def test_request_helper(self):
        self.assertEqual(method_request({'action': 'factory', 'name': 'X'})['method']['name'], 'X')
        r = method_request({'action': 'import_xml', 'xml': XML})
        self.assertEqual(r['unsupported'], [])
        self.assertIn('<analysisMethodName>Lab GF 175</analysisMethodName>',
                      method_request({'action': 'export_xml', 'method': r['method']})['xml'])
        with self.assertRaises(ValueError):
            method_request({'action': 'nope'})
        json.dumps(pullup_definition(factory_method(), {'blue': 'DATA1', 'orange': 'DATA105'}))


if __name__ == '__main__':
    unittest.main()


class EngineSettingsTests(unittest.TestCase):
    def setUp(self):
        from genemapper_open.demo import pipeline_demo_bytes, SYNTHETIC_PIPELINE_STANDARD_BP
        from genemapper_open.server import analyze
        self.run_analysis = lambda **kw: analyze(pipeline_demo_bytes(), 'p.fsa', analysis_engine='morphology_ratio',
            sizing_mode='auto', standard_channel='DATA106', standard_bp=list(SYNTHETIC_PIPELINE_STANDARD_BP), **kw)

    def test_channel_thresholds_detector_and_range(self):
        base = self.run_analysis()
        first = base['channels'][0]
        self.assertTrue(first['peaks'])
        self.assertNotIn('detector', base['peak_settings'])
        high = self.run_analysis(channel_thresholds={first['tag']: 1e9})
        self.assertEqual(high['channels'][0]['peaks'], [])
        self.assertEqual(high['peak_settings']['channel_thresholds'], {first['tag']: 1e9})
        self.assertEqual(high['sizing']['status'], 'accepted')
        detector = {'baseline_window': 51, 'smooth_passes': 2, 'window': 9, 'standard_window': 15,
                    'min_width': 2.0, 'tau_begin': 0.0, 'tau_end': 0.0}
        same = self.run_analysis(detector=json.dumps(detector))
        self.assertEqual([p['scan'] for p in same['channels'][0]['peaks']], [p['scan'] for p in first['peaks']])
        scans = sorted(p['scan'] for p in first['peaks'])
        cut = int(scans[0]) + 1
        ranged = self.run_analysis(analysis_range=(cut, len(first['values']) - 1))
        self.assertTrue(all(p['scan'] > cut for p in ranged['channels'][0]['peaks']))
        self.assertEqual(ranged['peak_settings']['analysis_range'], [cut, len(first['values']) - 1])
        for bad in ({'detector': {**detector, 'window': 8}}, {'detector': {'window': 9}},
                    {'channel_thresholds': {'X1': 5}}, {'channel_thresholds': {first['tag']: 0}}):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                self.run_analysis(**bad)
        from genemapper_open.server import _parse_analysis_range
        for bad in ('[5]', '[9,3]', '["a",4]'):
            with self.assertRaises(ValueError):
                _parse_analysis_range(bad)
