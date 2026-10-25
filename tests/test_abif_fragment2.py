import struct
import unittest

from genemapper_open.abif import AbifFile, AbifError
from genemapper_open.matrix import _entry, write_abif
from genemapper_open.analysis_method import method_request


def fsa(sver4, extra=(), lengths=(10, 10, 10, 10)):
    e = [_entry('SVER', 4, 2, 1, len(sver4), sver4.encode())]
    for k, n in enumerate(lengths, 1):
        e.append(_entry('DATA', k, 4, 2, n, struct.pack(f'>{n}h', *range(n))))
    e.extend(extra)
    return write_abif(e)


def ints(name, number, vals):
    return _entry(name, number, 5, 4, len(vals), struct.pack(f'>{len(vals)}i', *vals))


class Fragment2Tests(unittest.TestCase):
    def data(self, raw, tag='DATA1'):
        return next(c for c in AbifFile(raw, 'a.fsa').channels() if c['tag'] == tag)['values']

    def test_oversized_values_restored(self):
        raw = fsa('Fragment Format 2.0', [ints('OvrV', 1, [40000, 70000]), ints('OvrI', 1, [3, 7])])
        v = self.data(raw)
        self.assertEqual((v[3], v[7], v[4]), (40000, 70000, 4))
        # older format: overrides are not applied
        self.assertEqual(self.data(fsa('Fragment Format 1.0', [ints('OvrV', 1, [40000]), ints('OvrI', 1, [3])]))[3], 3)

    def test_edr_only_with_flag(self):
        extra = [ints('EDRV', 2, [99999]), ints('EDRI', 2, [0])]
        self.assertEqual(self.data(fsa('Fragment Format 2.0', extra), 'DATA2')[0], 0)
        flag = _entry('EDRF', 1, 4, 2, 1, struct.pack('>h', 1))
        self.assertEqual(self.data(fsa('Fragment Format 2.0', extra + [flag]), 'DATA2')[0], 99999)

    def test_invalid_overrides_rejected(self):
        with self.assertRaisesRegex(AbifError, 'different sizes'):
            AbifFile(fsa('Fragment Format 2.0', [ints('OvrV', 1, [1, 2]), ints('OvrI', 1, [3])]), 'a.fsa').channels()
        with self.assertRaisesRegex(AbifError, 'invalid indices'):
            AbifFile(fsa('Fragment Format 2.0', [ints('OvrV', 1, [1]), ints('OvrI', 1, [10])]), 'a.fsa').channels()

    def test_dyes_cut_to_shortest(self):
        chs = AbifFile(fsa('Fragment Format 1.0', lengths=(10, 8, 10, 9)), 'a.fsa').channels()
        self.assertEqual({len(c['values']) for c in chs if c['group'] == 'raw'}, {8})


class ShippedMethodTests(unittest.TestCase):
    def test_minifiler_method_from_id_x_defaults(self):
        ms = method_request({'action': 'shipped'})['methods']
        self.assertEqual([m['name'] for m in ms], ['MiniFiler_GS500_HID_v1'])
        self.assertEqual(ms[0]['allele']['bin_set'], 'AmpFLSTR_Bins_v1X')


if __name__ == '__main__':
    unittest.main()
