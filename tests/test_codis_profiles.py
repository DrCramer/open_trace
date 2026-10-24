"""CODIS CMF 3.2/3.3 export and Profile Manager / Profile Comparison."""
import unittest
import xml.etree.ElementTree as ET

from genemapper_open.codis import build_cmf, parse_codis_markers, parse_codis_kits, codis_kit_name, codis_request
from genemapper_open.profiles import (percent_match, concordance_groups, compare, export_csv, parse_profiles,
                                      export_profiles)

NS = '{urn:CODISImportFile-schema}'


def sample(name='S1', **kw):
    base = {'sample_name': name, 'specimen_id': '', 'category': 'Forensic, Unknown', 'panel': 'GlobalFiler_Panel_v1',
            'batch_id': 'PLATE1', 'case_id': 'C-1', 'comment': '', 'partial': '', 'source_id': '',
            'loci': [{'marker': 'D3S1358', 'alleles': ['15', '16']}, {'marker': 'AMEL', 'alleles': ['X', 'Y']},
                     {'marker': 'Yindel', 'alleles': ['2']}, {'marker': 'NOTCODIS', 'alleles': ['7']}]}
    base.update(kw)
    return base


def request(samples, version='3.2'):
    return {'version': version, 'source_lab': 'LAB01', 'destination_ori': 'ORI99', 'user': 'analyst',
            'submit_datetime': '2026-10-24T10:00:00', 'samples': samples}


class CodisTests(unittest.TestCase):
    def test_shipped_configuration(self):
        mapping, limits = parse_codis_markers()
        self.assertEqual(mapping['AMEL'], 'Amelogenin')
        self.assertEqual(mapping['B_DYS389I'], 'DYS389 I')
        self.assertEqual(mapping['D3S1358'], 'D3S1358')
        self.assertEqual(limits, {'TOTAL_MARKER_FOR_CMF1.0': 64, 'TOTAL_MARKER_FOR_CMF3.x': 64, 'YMARKER': 32, 'STRMARKER': 32})
        kits = parse_codis_kits()
        self.assertEqual(codis_kit_name('GlobalFiler_Panel_v1', kits), 'GlobalFiler')
        self.assertEqual(codis_kit_name('GlobalFiler_Express_Panel_v1', kits), 'GlobalFiler Express')
        self.assertEqual(codis_kit_name('Identifiler_Plus_Panel_v1', kits), 'Identifiler Plus')
        self.assertEqual(codis_kit_name('MyKit', kits), 'MyKit')
        with self.assertRaises(ValueError):
            parse_codis_markers('A\na')
        self.assertIn('Forensic, Unknown', codis_request({'action': 'config'})['default_specimen_types'])

    def test_cmf32_structure(self):
        out = build_cmf(request([sample(partial='True', comment='blood swab')]))
        root = ET.fromstring(out['xml'])
        self.assertEqual(root.tag, NS + 'CODISImportFile')
        self.assertEqual([c.tag[len(NS):] for c in root][:6],
                         ['HEADERVERSION', 'MESSAGETYPE', 'DESTINATIONORI', 'SOURCELAB', 'SUBMITBYUSERID', 'SUBMITDATETIME'])
        self.assertEqual(root.find(NS + 'HEADERVERSION').text, '3.2')
        spec = root.find(NS + 'SPECIMEN')
        self.assertEqual(spec.get('CASEID'), 'C-1')
        self.assertEqual(spec.get('PARTIAL'), 'true')
        self.assertIsNone(spec.get('NCIC'))
        self.assertEqual(spec.find(NS + 'SPECIMENID').text, 'S1')
        self.assertEqual(spec.find(NS + 'SPECIMENCATEGORY').text, 'Forensic, Unknown')
        self.assertEqual(spec.find(NS + 'SPECIMENCOMMENT').text, 'blood swab')
        loci = spec.findall(NS + 'LOCUS')
        self.assertEqual([l.find(NS + 'LOCUSNAME').text for l in loci], ['D3S1358', 'Amelogenin', 'Yindel'])
        self.assertEqual(loci[0].get('KIT'), 'GlobalFiler')
        self.assertEqual(loci[0].get('BATCHID'), 'PLATE1')
        self.assertEqual([a.find(NS + 'ALLELEVALUE').text for a in loci[0].findall(NS + 'ALLELE')], ['15', '16'])
        self.assertEqual(loci[0].find(NS + 'READINGBY').text, 'analyst')
        self.assertEqual(out['skipped_markers'], ['NOTCODIS'])

    def test_cmf33_ncic_vicap_and_merge_of_kits(self):
        a = sample('A', specimen_id='SP1', ncic='M123456789', vicap='V12345678901')
        b = sample('B', specimen_id='SP1', ncic='M123456789', vicap='V12345678901', panel='Yfiler_Plus_Panel_v1',
                   loci=[{'marker': 'B_DYS389I', 'alleles': ['13']}, {'marker': 'D3S1358', 'alleles': ['16', '15']}])
        out = build_cmf(request([a, b], '3.3'))
        root = ET.fromstring(out['xml'])
        specs = root.findall(NS + 'SPECIMEN')
        self.assertEqual(len(specs), 1)
        self.assertEqual(specs[0].get('NCIC'), 'M123456789')
        names = [l.find(NS + 'LOCUSNAME').text for l in specs[0].findall(NS + 'LOCUS')]
        self.assertEqual(names, ['D3S1358', 'Amelogenin', 'Yindel', 'DYS389 I'])
        self.assertEqual(out['specimens'], 1)
        self.assertEqual(out['samples'], 2)

    def test_validation_messages(self):
        cases = [
            ([sample(loci=[{'marker': 'D3S1358', 'alleles': ['OL']}])], 'invalid for CODIS'),
            ([sample(loci=[])], 'at least one allele'),
            ([sample(category='No Export')], 'No Export'),
            ([sample(comment='a|b')], '|'),
            ([sample('A', specimen_id='X'), sample('B', specimen_id='X', case_id='C-2')], 'case identifiers'),
            ([sample('A', specimen_id='X'), sample('B', specimen_id='X', loci=[{'marker': 'D3S1358', 'alleles': ['14', '16']}])], 'Non-concordant'),
        ]
        for samples, text in cases:
            with self.subTest(text=text), self.assertRaises(ValueError) as ctx:
                build_cmf(request(samples))
            self.assertIn(text, str(ctx.exception))
        with self.assertRaises(ValueError):
            build_cmf(request([sample(ncic='SHORT')], '3.3'))
        with self.assertRaises(ValueError):
            build_cmf({**request([sample()]), 'source_lab': ''})
        with self.assertRaises(ValueError):
            build_cmf(request([sample()], '3.0'))


def prof(pid, **markers):
    return {'id': pid, 'markers': {k: list(v) for k, v in markers.items()}}


class ProfileTests(unittest.TestCase):
    def test_percent_match_direction_and_ignore(self):
        ref = prof('R', A=['1', '2'], B=['3', '4'])
        comp = prof('C', A=['1', '2'], B=['3'])
        self.assertEqual(percent_match(ref, comp)[:3], (75.0, 3, 4))
        self.assertEqual(percent_match(comp, ref)[:3], (100.0, 3, 3))
        self.assertEqual(percent_match(prof('R', A=['1', 'F']), prof('C', A=['1']), ignore=['F'])[0], 100.0)

    def test_concordance_groups_and_comparisons(self):
        s = [prof('s1', A=['1', '2'], B=['3']), prof('s2', A=['2', '1'], B=['3']), prof('s3', A=['1', '2', '5'], B=['3']),
             prof('s4', A=['1', '2', '5'], B=['3']), prof('s5', A=['7'], B=['8'])]
        groups = concordance_groups(s)
        self.assertEqual([(g['kind'], g['members']) for g in groups], [('single', ['s1', 's2']), ('mixed', ['s3', 's4'])])
        res = compare({'mode': 'samples', 'samples': s, 'threshold': 80})
        self.assertEqual([(r['reference'], r['comparison'], r['percent']) for r in res['rows']],
                         [('Single-source group 1', 'Mixed-source group 2', 100.0)])
        refs = [{'id': 'Victim', 'type': 'Lab Reference', 'markers': {'A': ['1', '2'], 'B': ['3']}}]
        res = compare({'mode': 'lab_reference', 'samples': s, 'profiles': refs, 'selected': ['s5', 's3']})
        self.assertEqual([(r['reference'], r['comparison'], r['percent']) for r in res['rows']], [('Victim', 's3', 100.0)])
        res = compare({'mode': 'control', 'samples': s, 'profiles': refs, 'threshold': 50})
        self.assertEqual({r['comparison'] for r in res['rows']}, {'Single-source group 1', 'Mixed-source group 2'})
        csv_text = export_csv(compare({'mode': 'samples', 'samples': [prof('a', A=['1', '2']), prof('b', A=['1', '9'])], 'threshold': 50})['rows'])
        self.assertIn('<2>', csv_text)
        with self.assertRaises(ValueError):
            compare({'mode': 'samples', 'samples': s, 'threshold': 40})

    def test_profile_file_round_trip(self):
        text = ('Profile ID\tUD7\tMarker\tDye\tAllele 1\tAllele 2\tSize 1\n'
                '9947A\tCustom Control\tD3S1358\tB\t14\t15\t112\n9947A\tCustom Control\tAMEL\tR\tX\t\t98\n'
                'Ref1\tLab Reference\tD3S1358\tB\t16\t\t112\n')
        ps = parse_profiles(text)
        self.assertEqual([(p['id'], p['type'], p['markers']) for p in ps],
                         [('9947A', 'Custom Control', {'D3S1358': ['14', '15'], 'AMEL': ['X']}), ('Ref1', 'Lab Reference', {'D3S1358': ['16']})])
        self.assertEqual(ps[0]['dye']['AMEL'], 'R')
        again = parse_profiles(export_profiles(ps))
        self.assertEqual([(p['id'], p['type'], p['markers']) for p in again], [(p['id'], p['type'], p['markers']) for p in ps])
        with self.assertRaises(ValueError):
            parse_profiles('Name,Allele 1\nx,1\n')


if __name__ == '__main__':
    unittest.main()
