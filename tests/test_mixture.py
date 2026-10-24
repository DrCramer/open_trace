"""Mixture Analysis: segregation, extraction, population databases and statistics (ID-X help examples)."""
import math
import unittest

from genemapper_open.mixture import (FACTORY_METHOD, phr_threshold, segregate, extract, eligible, parse_population_db,
                                     genotype_frequency, combined_frequency, rmp, rmp_single, cpi, mixture_request,
                                     validate_method)


def sample(markers, sid='S'):
    return {'id': sid, 'markers': [{'name': n, 'alleles': [{'label': l, 'height': h} for l, h in al]} for n, al in markers.items()]}


POP = """Module=STR_JFS Mixture Analysis Population Statistics,
Version=1.0,
Database=Test DB,
Data Source=Synthetic,
Population=Example,Theta=0.01
Marker=D3S1358,N=200
Allele,Frequency
15,0.3
16,0.2
17,0.2
Marker=vWA,N=100
Allele,Frequency
15,*
18,0.1
Marker=D16S539,N=200
Allele,Frequency
9,0.1
10,0.1
12,0.3
"""


class MethodTests(unittest.TestCase):
    def test_factory_and_threshold_lookup(self):
        m = validate_method(FACTORY_METHOD)
        self.assertEqual(m['phr'][0], [50, 150, 0.4])
        self.assertEqual(phr_threshold(m['phr'], 49, 50), 0.4)      # MIT-1 .. 150 -> first row
        self.assertEqual(phr_threshold(m['phr'], 200, 50), 0.54)
        self.assertEqual(phr_threshold(m['phr'], 1000, 50), 0.63)
        self.assertEqual(phr_threshold(m['phr'], 5000, 50), 0.75)
        self.assertEqual(phr_threshold(m['phr'], 10, 50), 0.75)      # below MIT-1: last row (original quirk)
        for bad in ({**FACTORY_METHOD, 'mit': 1}, {**FACTORY_METHOD, 'phr': [[50, 150, .4], [200, 300, .5]]}, {**FACTORY_METHOD, 'name': 'a/b'}):
            with self.assertRaises(ValueError):
                validate_method(bad)


class SegregationTests(unittest.TestCase):
    def test_rules(self):
        one = sample({'A': [('1', 1000), ('2', 900)], 'B': [('3', 800), ('4', 700), ('5', 600)]})
        self.assertEqual(segregate(one, FACTORY_METHOD)['contributors'], 1)
        two = sample({'A': [('1', 1000), ('2', 900), ('3', 300), ('4', 200)]})
        self.assertEqual(segregate(two, FACTORY_METHOD)['contributors'], 2)
        two_b = sample({'A': [('1', 1000), ('2', 900), ('3', 300)], 'B': [('5', 1000), ('6', 900), ('7', 300)]})
        self.assertEqual(segregate(two_b, FACTORY_METHOD)['reason'], 'several_three_allele_markers')
        phr = sample({'A': [('1', 1000), ('2', 300)]})
        self.assertEqual(segregate(phr, FACTORY_METHOD)['reason'], 'phr_fail:A')
        three = sample({'A': [(str(i), 500) for i in range(5)]})
        self.assertEqual(segregate(three, FACTORY_METHOD)['contributors'], 3)
        low = sample({'A': [('1', 1000), ('2', 900), ('3', 40), ('4', 30)]})
        r = segregate(low, FACTORY_METHOD)
        self.assertEqual((r['contributors'], r['adbi_markers']), (1, ['A']))
        self.assertEqual(eligible(sample({'A': [('OL', 500)]}))[0], False)
        self.assertEqual(eligible(sample({'A': [('OL<6', 500)]}))[0], True)


class ExtractionTests(unittest.TestCase):
    def test_four_peak_mx_residual_and_selection(self):
        # Major (1,2) ~ 0.8, minor (3,4) ~ 0.2 at two 4-peak markers; a 3-peak and a 2-peak marker.
        s = sample({'A': [('1', 800), ('2', 800), ('3', 200), ('4', 200)],
                    'B': [('5', 820), ('6', 780), ('7', 210), ('8', 190)],
                    'C': [('9', 800), ('10', 800), ('11', 400)],
                    'D': [('12', 1000), ('13', 1000)]})
        r = extract(s, FACTORY_METHOD)
        self.assertEqual(r['status'], 'extracted')
        self.assertAlmostEqual(r['average_mx'], 0.2, places=3)
        best = {}
        for row in r['rows']:
            if row['selected']:
                best.setdefault(row['marker'], []).append((tuple(row['c1']), tuple(row['c2'])))
        self.assertIn((('1', '2'), ('3', '4')), best['A'])
        self.assertIn((('5', '6'), ('7', '8')), best['B'])
        self.assertIn((('9', '10'), ('11', '11')), best['C'])
        a_row = next(x for x in r['rows'] if x['marker'] == 'A' and x['c1'] == ['1', '2'] and x['c2'] == ['3', '4'])
        self.assertAlmostEqual(a_row['residual'], 0.0, places=9)
        self.assertEqual((a_row['phr1'], a_row['phr2'], a_row['phr_status'], a_row['iq']), (1.0, 1.0, 'pass', 'pass'))
        # Help example: residual formula for (ab, cd) with Mx 0.2
        bad = next(x for x in r['rows'] if x['marker'] == 'A' and x['c1'] == ['3', '4'] and x['c2'] == ['1', '2'])
        self.assertEqual(bad['residual_status'], 'low')
        # (AB, AC) shared PHR example from the help: A=350, B=600, C=400 -> 0.35
        s2 = sample({'A': [('A', 350), ('B', 600), ('C', 400)], 'X': [('1', 800), ('2', 800), ('3', 200), ('4', 200)]})
        row = next(x for x in extract(s2, FACTORY_METHOD)['rows'] if x['marker'] == 'A' and x['c1'] == ['A', 'B'] and x['c2'] == ['A', 'C'])
        self.assertAlmostEqual(row['phr1'], 0.35)
        self.assertEqual(row['phr1'], row['phr2'])
        self.assertEqual(extract(sample({'A': [('1', 900), ('2', 300)]}), FACTORY_METHOD)['status'], 'no_mx')

    def test_known_match(self):
        s = sample({'A': [('1', 800), ('2', 800), ('3', 200), ('4', 200)], 'B': [('5', 820), ('6', 780), ('7', 210), ('8', 190)]})
        r = extract(s, FACTORY_METHOD, known={'A': ['3', '4'], 'B': ['7', '8']})
        self.assertEqual(r['known_match'], 'C2 (Minor)')


class StatisticsTests(unittest.TestCase):
    def setUp(self):
        self.db = parse_population_db(POP)
        self.pop = self.db['populations'][0]

    def test_population_db_and_genotype_frequencies(self):
        self.assertEqual(self.db['database'], 'Test DB')
        self.assertEqual(self.pop['theta'], 0.01)
        # help example: (15,16) 2pq = 0.12; (15,15) p^2 + p(1-p)theta = 0.0921; (15,F) 2p = 0.6
        self.assertAlmostEqual(genotype_frequency(self.pop, 'D3S1358', '15', '16'), 0.12)
        self.assertAlmostEqual(genotype_frequency(self.pop, 'D3S1358', '15', '15'), 0.0921)
        self.assertAlmostEqual(genotype_frequency(self.pop, 'D3S1358', '15', 'F'), 0.6)
        self.assertAlmostEqual(combined_frequency(self.pop, 'D3S1358', [['15', '15'], ['15', '16']]), 0.2121)
        self.assertAlmostEqual(combined_frequency(self.pop, 'D3S1358', [['15', '15'], ['15', '16'], ['15', 'F']]), 0.6)
        # minimum frequency 5/2N for '*' and unseen alleles
        self.assertAlmostEqual(genotype_frequency(self.pop, 'vWA', '15', '15'), 0.025 ** 2 + 0.025 * 0.975 * 0.01)
        with self.assertRaises(ValueError):
            parse_population_db('nonsense')

    def test_rmp_and_cpi(self):
        prof = {'D3S1358': {'genotypes': [['15', '16']]}, 'D16S539': {'genotypes': [['12', '12']]}, 'AMEL': {'genotypes': [['X', 'Y']]},
                'FGA': {'genotypes': [['20', '21']]}}
        r = rmp(prof, self.pop)
        expected = 0.12 * (0.09 + 0.3 * 0.7 * 0.01)
        self.assertAlmostEqual(r['profile_frequency'], expected)
        self.assertAlmostEqual(r['rmp'], 1 / expected)
        self.assertEqual(r['missing_markers'], ['FGA'])
        single = rmp_single({'D3S1358': ['15', '16', '17'], 'D16S539': ['12']}, self.pop, {'D16S539': '2p'})
        self.assertAlmostEqual(single['profile_frequency'], min(0.12, 0.12, 0.08) * 0.6)
        c = cpi({'D3S1358': ['15', '16', '17'], 'D16S539': ['9', '10', '12'], 'vWA': ['15', '18']}, self.pop, {'vWA'})
        self.assertAlmostEqual(c['cpi'], 0.7 ** 2 * 0.5 ** 2)
        self.assertAlmostEqual(c['cpe'], 1 - c['cpi'])
        self.assertEqual(c['details'][2]['inclusion'], 1.0)
        out = mixture_request({'action': 'statistics', 'kind': 'cpi', 'population_db': POP, 'markers': {'D3S1358': ['15', '16', '17']}})
        self.assertAlmostEqual(out['results'][0]['cpi'], 0.49)


if __name__ == '__main__':
    unittest.main()
