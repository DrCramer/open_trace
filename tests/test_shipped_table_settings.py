import json
import socket
import unittest
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

DATA = Path(__file__).resolve().parent.parent / 'genemapper_open' / 'data' / 'shipped_table_settings'

EXPECTED = {
    '310 Data Analysis', '31XX Data Analysis', '31XX Data Analysis_6Dye',
    '3500 Data Analysis', '3500 Data Analysis_6Dye', 'CODIS Export',
    'Import Reference Profiles', 'Known Sample Export', 'Traditional Allele Table',
    'VALID_GMIDX_TableSetting-1.0', 'View CGQ Overrides', 'View Edited Samples',
    'View Unedited Samples', 'Yfiler Haplotype DB Export',
}


class ShippedTableSettingsData(unittest.TestCase):
    def test_all_fourteen_present_and_well_formed(self):
        files = sorted(DATA.glob('*.xml'))
        self.assertEqual({p.stem for p in files}, EXPECTED)
        for p in files:
            root = ET.fromstring(p.read_text(encoding='utf-8'))
            self.assertTrue(root.tag.endswith('TableSettingContainer'), p.name)
            self.assertIsNotNone(root.find('.//xmlProjectTableProfile'), p.name)
            # No DTD/entity injection in shipped data.
            self.assertNotIn('<!DOCTYPE', p.read_text(encoding='utf-8'))


class ShippedTableSettingsRoute(unittest.TestCase):
    def test_route_serves_every_setting(self):
        import genemapper_open.server as S
        import threading
        # Exercise serve() on a background thread over an ephemeral port.
        port = _free_port()
        t = threading.Thread(target=S.serve, kwargs={'port': port,
             'projects_dir': '/tmp/claude-0/tsdefaults/proj',
             'security_dir': '/tmp/claude-0/tsdefaults/sec'}, daemon=True)
        t.start()
        base = f'http://127.0.0.1:{port}'
        _wait(base)
        req = urllib.request.Request(base + '/api/shipped-table-settings',
                                     headers={'Host': f'127.0.0.1:{port}'})
        body = json.load(urllib.request.urlopen(req, timeout=5))
        names = {s['name'] for s in body['settings']}
        self.assertEqual(names, EXPECTED)
        for s in body['settings']:
            self.assertIn('TableSettingContainer', s['xml'])


def _free_port():
    s = socket.socket(); s.bind(('127.0.0.1', 0)); p = s.getsockname()[1]; s.close(); return p


def _wait(base):
    import time
    for _ in range(100):
        try:
            urllib.request.urlopen(urllib.request.Request(
                base + '/api/health', headers={'Host': base.split('//')[1]}), timeout=1)
            return
        except Exception:
            time.sleep(0.05)



if __name__ == '__main__':
    unittest.main()
