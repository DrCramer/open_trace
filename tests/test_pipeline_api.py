"""Public morphology/ratio API checks against independent synthetic migration."""
import json
import math
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import re
import urllib.error
import urllib.parse
import urllib.request

from genemapper_open.calibration import CalibrationError
from genemapper_open.demo import build_abif, pipeline_demo_bytes, SYNTHETIC_PIPELINE_STANDARD_BP
from genemapper_open.server import analyze

EXPECTED = [40,70,110,145,195,260]


def synthetic(large_second=False):
    centers = [200+20*bp for bp in EXPECTED]
    heights = [1000,15000 if large_second else 1000,1000,1000,1000,1000]
    standard = [round(20+sum(h*math.exp(-0.5*((i-center)/5)**2)
                            for center,h in zip(centers, heights))) for i in range(6501)]
    other = [round(10+800*math.exp(-0.5*((i-3800)/5)**2)) for i in range(6501)]
    items = [(b'DATA',1,4,2,len(standard),struct.pack(f'>{len(standard)}h',*standard)),
             (b'DATA',2,4,2,len(other),struct.pack(f'>{len(other)}h',*other)),
             (b'DATA',5,4,2,3,struct.pack('>3h',1,2,3)),
             (b'DATA',9,4,2,len(other),struct.pack(f'>{len(other)}h',*other))]
    return build_abif(items)


OPTIONS = {'analysis_engine':'morphology_ratio', 'sizing_mode':'auto',
           'standard_channel':'DATA1', 'standard_bp':EXPECTED}


class PipelineApiTests(unittest.TestCase):
    def test_public_pipeline_sizes_reference_and_other_raw_dye(self):
        result = analyze(synthetic(), 'invented.fsa', **OPTIONS)
        sizing = result['sizing']
        self.assertEqual(sizing['status'], 'accepted', sizing)
        self.assertEqual(sizing['analysis_engine'], 'morphology_ratio')
        self.assertGreater(sizing['base_quality']['score'], 0.99)
        self.assertEqual(sizing['settings']['selected_data_group'], 'raw')
        self.assertTrue(sizing['calibration_applied'])
        self.assertTrue(all(not p['tentative'] for p in sizing['matched']))
        reference, other, auxiliary, analyzed = result['channels']
        self.assertEqual(len(reference['peaks']), 6)
        self.assertEqual(len(other['peaks']), 1)
        self.assertAlmostEqual(other['peaks'][0]['size_bp'], 180, delta=0.1)
        self.assertGreater(other['peaks'][0]['raw_height'], other['peaks'][0]['height'])
        self.assertEqual(other['peaks'][0]['width_scans'], other['peaks'][0]['fwhm_scan'])
        self.assertEqual(auxiliary['peaks'], [])
        self.assertEqual(analyzed['peaks'], [])
        self.assertEqual(auxiliary['processing']['status'], 'skipped')
        self.assertEqual(result['peak_settings']['min_distance'], None)
        self.assertNotIn('control_signal', json.dumps(sizing))
        json.dumps(result, allow_nan=False)

    def test_quality_rejection_withholds_all_bp_after_ratio_acceptance(self):
        result = analyze(synthetic(True), 'invented.fsa', **OPTIONS)
        self.assertEqual(result['sizing']['matching']['status'], 'accepted')
        self.assertEqual(result['sizing']['base_quality']['score'], 0)
        self.assertEqual(result['sizing']['status'], 'rejected')
        self.assertEqual(result['sizing']['anchors'], [])
        self.assertEqual(result['calibration'], [])
        self.assertTrue(all(p['tentative'] for p in result['sizing']['matched']))
        self.assertTrue(all('size_bp' not in p for ch in result['channels'] for p in ch['peaks']))

    def test_curve_failure_after_qc_acceptance_withholds_all_bp(self):
        with patch('genemapper_open.calibration.build_calibration', side_effect=CalibrationError('forced invalid curve')):
            result = analyze(synthetic(), 'invented.fsa', **OPTIONS)
        self.assertGreater(result['sizing']['base_quality']['score'], 0.99)
        self.assertEqual(result['sizing']['status'], 'rejected')
        self.assertEqual(result['sizing']['stage'], 'calibration_model')
        self.assertEqual(result['sizing']['anchors'], [])
        self.assertFalse(result['sizing']['calibration_applied'])
        self.assertTrue(all('size_bp' not in p for ch in result['channels'] for p in ch['peaks']))

    def test_autoheight_and_three_marker_text_are_explicit(self):
        result = analyze(synthetic(), 'invented.fsa', height_mode='basic_standard_auto', **OPTIONS)
        self.assertEqual(result['sizing']['autoheight']['threshold'], 30)
        self.assertEqual(result['sizing']['status'], 'accepted')
        result = analyze(synthetic(), 'invented.fsa', **(OPTIONS|{'standard_bp':'40,70,110'}))
        self.assertEqual(result['sizing']['expected_count'], 3)
        self.assertEqual(result['sizing']['expected_bp'], [40.0,70.0,110.0])

    def test_invalid_new_configuration_is_not_treated_as_data_rejection(self):
        cases = ({'analysis_engine':'unknown'}, {'height_mode':'guess'}, {'quality_threshold':0},
                 {'quality_threshold':float('nan')}, {'quality_threshold':True},
                 {'num_extra_peaks':True}, {'num_extra_peaks':33}, {'threshold':0},
                 {'standard_start':1}, {'standard_end':6400}, {'standard_channel':'DATA5'},
                 {'standard_bp':'40,40,110'}, {'standard_bp':[40,float('inf'),110]})
        for extra in cases:
            options = OPTIONS|extra
            threshold = options.pop('threshold',100)
            with self.subTest(extra=extra), self.assertRaises(ValueError):
                analyze(synthetic(), 'invented.fsa', threshold=threshold, **options)

    def test_new_detection_is_available_without_automatic_sizing(self):
        result = analyze(synthetic(), 'invented.fsa', analysis_engine='morphology_ratio')
        self.assertEqual(result['sizing']['status'], 'not_requested')
        self.assertEqual(result['calibration'], [])
        self.assertEqual(result['channels'][0]['processing']['detector']['window'],9)
        self.assertTrue(all('size_bp' not in p for ch in result['channels'] for p in ch['peaks']))

    def test_cli_report_contains_diagnostics_and_omits_raw_channels(self):
        with tempfile.TemporaryDirectory() as directory:
            source, report = Path(directory)/'invented.fsa', Path(directory)/'report.json'
            source.write_bytes(synthetic())
            process = subprocess.run([sys.executable,'-m','genemapper_open','size',str(source),
                '--analysis-engine','morphology_ratio','--standard-channel','DATA1',
                '--standard-bp',','.join(map(str,EXPECTED)),'--height-mode','basic_standard_auto',
                '--output',str(report)], capture_output=True, text=True, timeout=20)
            self.assertEqual(process.returncode,0,process.stderr)
            output = json.loads(report.read_text(encoding='utf-8'))
            self.assertNotIn('channels', output)
            self.assertNotIn('values', output)
            self.assertEqual(output['sizing']['status'],'accepted')
            self.assertIn('base_quality', output['sizing'])
        from genemapper_open import __version__
        self.assertEqual(output['analysis_version'],'OpenTrace/'+__version__)

    def test_distributed_pipeline_demo_has_independent_180bp_peak(self):
        result = analyze(pipeline_demo_bytes(), 'synthetic-pipeline.fsa', analysis_engine='morphology_ratio',
            sizing_mode='auto', standard_channel='DATA106', standard_bp=list(SYNTHETIC_PIPELINE_STANDARD_BP))
        self.assertEqual(result['sizing']['status'],'accepted')
        for channel in result['channels'][:-1]:
            known_peak = next(p for p in channel['peaks'] if abs(p['scan']-3800) <= 1)
            self.assertAlmostEqual(known_peak['size_bp'],180,delta=0.1)

    def test_cli_pipeline_demo_and_mutually_exclusive_old_demo(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)/'demo.fsa'
            command = [sys.executable,'-m','genemapper_open','demo',str(output),'--pipeline']
            result = subprocess.run(command,capture_output=True,text=True,timeout=20)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(output.read_bytes(),pipeline_demo_bytes())
            invalid = subprocess.run(command+['--sizing'],capture_output=True,text=True,timeout=20)
            self.assertEqual(invalid.returncode,2)

    def test_http_routes_new_settings_and_returns_input_errors(self):
        process = subprocess.Popen([sys.executable,'-u','-m','genemapper_open','serve','--port','0'],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding='utf-8')
        try:
            line = process.stdout.readline()
            base = re.search(r'http://127\.0\.0\.1:\d+/',line).group(0)
            with urllib.request.urlopen(base,timeout=5) as response:
                token = re.search(r"const TOKEN='([^']+)'",response.read().decode()).group(1)
            with urllib.request.urlopen(base+'api/health',timeout=5) as response:
                from genemapper_open import __version__
                self.assertEqual(json.load(response)['analysis_version'],'OpenTrace/'+__version__)
            options = OPTIONS|{'standard_bp':','.join(map(str,EXPECTED)), 'height_mode':'basic_standard_auto'}
            request = urllib.request.Request(base+'api/analyze?'+urllib.parse.urlencode(options),
                data=synthetic(), headers={'X-Session-Token':token}, method='POST')
            with urllib.request.urlopen(request,timeout=10) as response:
                self.assertEqual(json.load(response)['sizing']['status'],'accepted')
            demo_options = options|{'standard_channel':'DATA106'}
            request = urllib.request.Request(base+'api/demo-pipeline?'+urllib.parse.urlencode(demo_options),
                data=b'',headers={'X-Session-Token':token},method='POST')
            with urllib.request.urlopen(request,timeout=10) as response:
                demo = json.load(response)
            self.assertEqual(demo['sizing']['status'],'accepted')
            known = next(p for p in demo['channels'][0]['peaks'] if abs(p['scan']-3800) <= 1)
            self.assertAlmostEqual(known['size_bp'],180,delta=0.1)
            request = urllib.request.Request(base+'api/analyze?'+urllib.parse.urlencode(options|{'quality_threshold':'nan'}),
                data=synthetic(), headers={'X-Session-Token':token}, method='POST')
            with self.assertRaises(urllib.error.HTTPError) as raised:
                urllib.request.urlopen(request,timeout=10)
            with raised.exception as response:
                self.assertEqual(response.code,400)
                self.assertIn('quality_threshold',json.load(response)['error'])
        finally:
            process.terminate()
            process.wait(timeout=5)
            process.stdout.close()
            process.stderr.close()


if __name__ == '__main__':
    unittest.main()
