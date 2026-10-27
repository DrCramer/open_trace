import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from genemapper_open.gm_kit_import import (KitFormatError, build_kit, default_ladder_quality_definition,
    import_kit, import_request, list_panels, parse_bins, parse_panels, parse_stutter)
from genemapper_open.kit_definition import load_kit_definition

# Synthetic files in the observed tab layout; no manufacturer definitions.
PANELS = '\r\n'.join([
    'Version\tGMID-X v 1.4', 'Kit type:\tMICROSATELLITE', 'Chemistry Kit\tTestKit',
    'Panel\tTest_Panel_v1',
    'M1\tblue\t100.0\t140.0\t12,14\t4\tnone\tfalse\tfalse\t10,11,12,13,14,15,',
    'M2\tgreen\t150.0\t190.0\t8\t4\tnone\t8,9,10,',
    'Y1\t\tyellow\t200.0\t240.0\t20\t4\tnone\ttrue\t19,20,21,',
]) + '\r\n'
BINS = '\n'.join([
    'Version\tGMID-X v 1.4', 'Chemistry Kit\tTestKit', 'BinSet Name\tTest_Bins_v1',
    'Panel Name\tTest_Panel_v1',
    'Marker Name\tM1',
    '10\t104.0\t0.5\t0.5', '11\t108.0\t0.5\t0.5', '12\t112.0\t0.5\t0.5', '12.2\t114.0\t0.4\t0.4\tvirtual',
    '13\t116.0\t0.5\t0.5', '14\t120.0\t0.5\t0.5', '15\t124.0\t0.5\t0.5', '99\t300.0\t0.5\t0.5',
    'Marker Name\tM2', '8\t160.0\t0.5\t0.5', '9\t164.0\t0.5\t0.5', '10\t168.0\t0.5\t0.5',
    'Marker Name\tY1', '19\t210.0\t0.5\t0.5', '20\t214.0\t0.5\t0.5',
])
STUTTER = '\n'.join([
    'Version\tSSFv1.2', 'Panel Name\tTest_Panel_v1',
    'Marker Name\tM1', '0.12\t3.25\t4.75\tMinus', '0.02\t3.25\t4.75\tPlus',
])


class ParseTests(unittest.TestCase):
    def test_quoted_ladder_column_keeps_first_and_last_alleles(self):
        # Observed PanGlobal layout: the ladder column is double-quoted with a trailing comma.
        panels = PANELS.replace('\t10,11,12,13,14,15,\r\n', '\t"10, 11, 12, 13, 14, 15, "\r\n')
        self.assertIn('"10, 11', panels)
        kit = import_kit(panels, BINS, STUTTER)
        m1 = {m['name']: m for m in kit['markers']}['M1']
        self.assertEqual(m1['ladder_labels'], ['10', '11', '12', '13', '14', '15'])
        self.assertFalse([d for d in kit['diagnostics'] if d.get('marker') == 'M1' and d['code'] == 'ladder_alleles_without_bins'])

    def test_panels_variants_and_flags(self):
        doc = parse_panels(PANELS)
        self.assertEqual(doc['version'], 'GMID-X v 1.4')
        self.assertEqual(list_panels(doc), [{'kit': 'TestKit', 'panel': 'Test_Panel_v1', 'markers': 3}])
        m1, m2, y1 = doc['kits'][0]['panels'][0]['markers']
        self.assertEqual((m1['dye'], m1['min_bp'], m1['repeat']), ('blue', 100.0, 4))
        self.assertEqual(m1['control_alleles'], ['12', '14'])
        self.assertEqual((m1['y_marker'], m1['internal_qc']), (False, False))
        self.assertEqual(m1['ladder_alleles'], ['10', '11', '12', '13', '14', '15'])
        self.assertFalse(m2['y_marker']); self.assertNotIn('internal_qc', m2)
        self.assertEqual(m2['ladder_alleles'], ['8', '9', '10'])
        self.assertEqual((y1['dye'], y1['y_marker']), ('yellow', True))  # stray empty column tolerated

    def test_bins_virtual_and_stutter(self):
        bins = parse_bins(BINS.encode('utf-8'))
        self.assertEqual(bins['bin_set'], 'Test_Bins_v1')
        m1 = bins['panels']['Test_Panel_v1']['M1']
        self.assertEqual([b['label'] for b in m1 if b['virtual']], ['12.2'])
        st = parse_stutter(STUTTER)['panels']['Test_Panel_v1']['M1']
        self.assertEqual([(s['direction'], s['ratio']) for s in st], [('minus', 0.12), ('plus', 0.02)])

    def test_malformed_inputs_are_rejected(self):
        bad = [
            (parse_panels, ''), (parse_panels, 'Version\tx\nKit type:\tSNP\n'),
            (parse_panels, 'Version\tx\nM1\tblue\t1\t2\tnone\t4\tnone\t1,\n'),
            (parse_panels, PANELS.replace('\t4\tnone\tfalse\tfalse', '\tfour\tnone\tfalse\tfalse')),
            (parse_panels, PANELS.replace('100.0', 'nan')),
            (parse_panels, b'x' * (2 * 1024 * 1024 + 1)),
            (parse_bins, 'Version\tx\nMarker Name\tM1\n'),
            (parse_bins, BINS.replace('\tvirtual', '\tweird')),
            (parse_stutter, STUTTER.replace('Minus', 'Sideways')),
            (parse_stutter, STUTTER.replace('0.12', '1.5')),
        ]
        for parse, text in bad:
            with self.subTest(parse=parse.__name__, text=str(text)[:40]):
                with self.assertRaises(KitFormatError):
                    parse(text)
        self.assertTrue(issubclass(KitFormatError, ValueError))


class BuildTests(unittest.TestCase):
    def test_build_kit_channels_bins_ladder_and_stutter(self):
        kit = import_kit(PANELS, BINS, STUTTER)
        self.assertIs(kit, load_kit_definition(kit) and kit)
        markers = {m['name']: m for m in kit['markers']}
        m1 = markers['M1']
        self.assertEqual((m1['channel'], m1['dye']), ('DATA1', 'blue'))
        self.assertNotIn('99', [b['label'] for b in m1['bins']])
        self.assertTrue(next(b for b in m1['bins'] if b['label'] == '12.2')['virtual'])
        self.assertEqual(m1['parameters']['backward_stutter_ranges'], [{'min_bp': 3.25, 'max_bp': 4.75, 'ratio': 0.12}])
        self.assertEqual(m1['parameters']['forward_stutter_ranges'], [{'min_bp': 3.25, 'max_bp': 4.75, 'ratio': 0.02}])
        self.assertEqual(markers['M2']['parameters']['backward_stutter_ranges'], [])  # global ratio 0
        self.assertEqual(markers['Y1']['ladder_labels'], ['19', '20'])
        self.assertTrue(markers['Y1']['y_marker'])
        codes = [d['code'] for d in kit['diagnostics']]
        self.assertIn('bins_outside_marker_range_excluded', codes)
        missing = next(d for d in kit['diagnostics'] if d['code'] == 'ladder_alleles_without_bins')
        self.assertEqual((missing['marker'], missing['labels']), ('Y1', ['21']))
        origin = next(d for d in kit['diagnostics'] if d['code'] == 'imported_from_genemapper_text')
        self.assertEqual((origin['chemistry_kit'], origin['bin_set'], origin['stutter_version']),
                         ('TestKit', 'Test_Bins_v1', 'SSFv1.2'))

    def test_method_and_channel_overrides(self):
        kit = import_kit(PANELS, BINS, None, method={'global_minus_stutter': {'ratio': 0.1},
                         'use_marker_specific_stutter': False}, dye_channels={'blue': 'DATA2'})
        m1 = kit['markers'][0]
        self.assertEqual(m1['channel'], 'DATA2')
        self.assertEqual(m1['parameters']['backward_stutter_ranges'], [{'min_bp': 3.25, 'max_bp': 4.75, 'ratio': 0.1}])
        with self.assertRaises(ValueError):
            import_kit(PANELS, BINS, method={'bogus': 1})
        with self.assertRaises(ValueError):
            import_kit(PANELS, BINS, panel='Nope')
        two = PANELS + 'Panel\tOther\r\nM1\tblue\t100\t140\t12\t4\tnone\t10,\r\n'
        with self.assertRaises(ValueError):
            build_kit(parse_panels(two), parse_bins(BINS))
        with self.assertRaises(ValueError):
            build_kit(parse_panels(two), parse_bins(BINS), panel='Other')  # no bins for that panel

    def test_default_ladder_quality_and_request(self):
        kit = import_kit(PANELS, BINS)
        lq = default_ladder_quality_definition(kit)
        self.assertEqual(sorted(lq['markers']), ['M1', 'M2', 'Y1'])
        self.assertEqual(lq['markers']['M1']['min_height_ratio'], 0.15)
        self.assertEqual(lq['markers']['M1']['spacing']['curve'], [{'distance_bp': 0, 'fraction': 0.25, 'interpolate': False}])
        listing = import_request({'panels': PANELS})
        self.assertIsNone(listing['kit']); self.assertEqual(len(listing['panel_list']), 1)
        full = import_request({'panels': PANELS, 'bins': BINS, 'stutter': STUTTER, 'panel': 'Test_Panel_v1'})
        self.assertEqual(full['kit']['name'], 'Test_Panel_v1')
        self.assertEqual(full['ladder_quality_definition'], lq)
        with self.assertRaises(ValueError):
            import_request({'panels': PANELS, 'panel': 'Test_Panel_v1'})
        with self.assertRaises(ValueError):
            import_request([])

    def test_kit_schema_optional_fields_validated(self):
        kit = import_kit(PANELS, BINS)
        doc = {'schema_version': 1, 'name': kit['name'], 'markers': [dict(kit['markers'][0])]}
        for field, value in (('y_marker', 'yes'), ('internal_qc', 1), ('control_alleles', '12'), ('dye', 5)):
            bad = json.loads(json.dumps(doc)); bad['markers'][0][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                load_kit_definition(bad)
        bad = json.loads(json.dumps(doc)); bad['markers'][0]['bins'][0]['virtual'] = 'no'
        with self.assertRaises(ValueError):
            load_kit_definition(bad)


class CliTests(unittest.TestCase):
    def test_import_kit_cli(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name, text in (('p.txt', PANELS), ('b.txt', BINS), ('s.txt', STUTTER)):
                (root / name).write_text(text, encoding='utf-8', newline='')
            listed = subprocess.run([sys.executable, '-m', 'genemapper_open', 'import-kit', str(root / 'p.txt'), '--list'],
                                    capture_output=True, text=True, check=True)
            self.assertIn('Test_Panel_v1', listed.stdout)
            subprocess.run([sys.executable, '-m', 'genemapper_open', 'import-kit', str(root / 'p.txt'), str(root / 'b.txt'),
                            '--stutter', str(root / 's.txt'), '-o', str(root / 'kit.json'),
                            '--ladder-quality', str(root / 'lq.json')], capture_output=True, text=True, check=True)
            kit = json.loads((root / 'kit.json').read_text(encoding='utf-8'))
            self.assertEqual(len(kit['markers']), 3)
            self.assertIn('M1', json.loads((root / 'lq.json').read_text(encoding='utf-8'))['markers'])
            failed = subprocess.run([sys.executable, '-m', 'genemapper_open', 'import-kit', str(root / 'b.txt')],
                                    capture_output=True, text=True)
            self.assertNotEqual(failed.returncode, 0)


class HttpTests(unittest.TestCase):
    def test_import_route(self):
        import re, urllib.request, urllib.error
        log = tempfile.TemporaryFile(mode='w+', encoding='utf-8')
        proc = subprocess.Popen([sys.executable, '-u', '-m', 'genemapper_open', 'serve', '--port', '0'],
                                stdout=subprocess.PIPE, stderr=log, text=True, encoding='utf-8')
        try:
            base = re.search(r'http://127\.0\.0\.1:\d+/', proc.stdout.readline()).group(0)
            with urllib.request.urlopen(base, timeout=5) as r:
                token = re.search(r"const TOKEN='([^']+)'", r.read().decode()).group(1)
            def post(payload, expected):
                req = urllib.request.Request(base + 'api/import-gm-kit', data=json.dumps(payload).encode(),
                    headers={'X-Session-Token': token, 'Content-Type': 'application/json'}, method='POST')
                try: resp = urllib.request.urlopen(req, timeout=15)
                except urllib.error.HTTPError as e: resp = e
                with resp:
                    self.assertEqual(resp.code, expected); return json.load(resp)
            out = post({'panels': PANELS, 'bins': BINS, 'stutter': STUTTER, 'panel': 'Test_Panel_v1'}, 200)
            self.assertEqual(len(out['kit']['markers']), 3)
            self.assertIn('M1', out['ladder_quality_definition']['markers'])
            self.assertIn('error', post({'panels': 'garbage'}, 400))
            self.assertIn('error', post([], 400))
        finally:
            proc.terminate(); proc.wait(timeout=5); proc.stdout.close(); log.close()


if __name__ == '__main__':
    unittest.main()
