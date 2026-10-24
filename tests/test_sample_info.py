"""Sample Info / Raw Data / EPT Data read from FSA files."""
import glob
import unittest
from pathlib import Path

from genemapper_open.sample_info import sample_info
from genemapper_open.demo import demo_bytes

_DIRS = [Path(__file__).resolve().parents[2] / 'research/extracted/payload/AppliedBiosystems/GeneMapperID-X/Example Data/Mixture Analysis/MA_SGM',
         Path('/home/claude/mix/Mixture Analysis/MA_SGM')]
REAL = next((sorted(str(f) for f in d.glob('*.fsa')) for d in _DIRS if d.is_dir() and any(d.glob('*.fsa'))), [])


class SampleInfoTests(unittest.TestCase):
    def test_synthetic_file(self):
        r = sample_info(demo_bytes(), 'demo.fsa')
        self.assertEqual(r['filename'], 'demo.fsa')
        self.assertTrue(r['raw'])
        self.assertEqual(r['scan_count'], len(r['raw'][0]['values']))
        ids = [s['id'] for s in r['sections']]
        self.assertIn('sample', ids)

    @unittest.skipUnless(REAL, 'ID-X example data not available')
    def test_3130_example(self):
        r = sample_info(Path(REAL[0]).read_bytes(), Path(REAL[0]).name)
        items = {i['key']: i['value'] for s in r['sections'] for i in s['items']}
        self.assertEqual(items['instrumentType'], '3130xl')
        self.assertEqual(items['dyeSetName'], 'F')
        self.assertEqual(items['runDuration'], '0:40:00')
        self.assertEqual(items['temperature'], 60)
        self.assertEqual(items['capillaryLength'], 36)
        self.assertEqual([c['dye'] for c in r['raw']], ['5-FAM', 'JOE', 'NED', 'ROX'])
        self.assertEqual(set(r['ept']), {'voltage', 'current', 'power', 'temperature'})
        self.assertEqual(r['ept']['temperature']['max'], 60)


if __name__ == '__main__':
    unittest.main()
