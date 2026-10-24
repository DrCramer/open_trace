import base64
import math
import random
import struct
import unittest

from genemapper_open import matrix as mx
from genemapper_open.abif import AbifFile

M_TRUE = [[1.0, 0.35, 0.05, 0.01], [0.6, 1.0, 0.4, 0.05], [0.1, 0.5, 1.0, 0.3], [0.02, 0.1, 0.55, 1.0]]


def dye_trace(rng, n=4000, spacing=180):
    dye = [0.0] * n
    for c in range(300, n - 200, spacing):
        h = rng.uniform(800, 3000)
        for t in range(c - 20, c + 21):
            dye[t] += h * math.exp(-((t - c) / 4.0) ** 2)
    return dye


def standard_traces(i, rng, n=4000):
    dye = dye_trace(rng, n)
    return [[int(M_TRUE[j][i] * dye[t] + 200 + rng.gauss(0, 3) + 0.0005 * t) for t in range(n)] for j in range(4)]


def fsa_310(traces, model='310 '):
    entries = [mx._entry('MODL', 1, 2, 1, len(model), model.encode('ascii'))]
    for k, tr in enumerate(traces, 1):
        entries.append(mx._entry('DATA', k, 4, 2, len(tr), struct.pack(f'>{len(tr)}h', *tr)))
    return mx.write_abif(entries)


class MatrixMakerTests(unittest.TestCase):
    def test_recovers_bleed_through_matrix(self):
        rng = random.Random(1)
        stds = [{'traces': standard_traces(i, rng), 'start': 200, 'points': 3600, 'filename': f's{i}'} for i in range(4)]
        m = mx.make_matrix(stds, 4)
        for r in range(4):
            self.assertEqual(m[r][r], 1.0)
            for c in range(4):
                self.assertAlmostEqual(m[r][c], M_TRUE[r][c], delta=0.003)
                self.assertEqual(m[r][c], round(m[r][c], 4))

    def test_window_and_peak_requirements(self):
        rng = random.Random(2)
        stds = [{'traces': standard_traces(i, rng), 'start': 200, 'points': 3600, 'filename': f's{i}'} for i in range(4)]
        short = [dict(s) for s in stds]
        short[2]['points'] = 150
        with self.assertRaises(mx.MatrixError) as e:
            mx.make_matrix(short, 4)
        self.assertEqual(e.exception.code, 'points')
        flat = [dict(s) for s in stds]
        flat[1] = {**flat[1], 'traces': [[200] * 4000 for _ in range(4)]}
        with self.assertRaises(mx.MatrixError) as e:
            mx.make_matrix(flat, 4)
        self.assertEqual(e.exception.code, 'peaks')
        with self.assertRaises(mx.MatrixError) as e:
            mx.make_matrix(stds[:3], 4)
        self.assertEqual(e.exception.code, 'samples')

    def test_medfit_is_robust_to_outliers(self):
        xs = [float(x) for x in range(100, 1100, 10)]
        ys = [0.25 * x + 3 for x in xs]
        ys[5] += 5000
        ys[40] -= 3000
        a, b, _ = mx.medfit(xs, ys)
        self.assertAlmostEqual(b, 0.25, delta=1e-3)

    def test_baseline_removes_offset_and_keeps_peaks(self):
        sig = [500 + int(1000 * math.exp(-((t - 900) / 5.0) ** 2)) for t in range(2000)]
        out = mx.genescan_baseline(sig)
        self.assertTrue(all(v >= 0 for v in out))
        self.assertLess(max(out[:500]), 5)
        self.assertGreater(out[900], 900)

    def test_smoothing_integer_arithmetic(self):
        self.assertEqual(mx.smooth7([0, 0, 0, 21, 0, 0, 0]), [0, 0, 0, 7, 0, 0, 0])
        self.assertEqual(mx.smooth7([0, 0, 0, -10, 0, 0, 0])[3], -3)  # Java truncation toward zero


class MatrixFileTests(unittest.TestCase):
    def test_mtx_round_trip(self):
        data = mx.matrix_to_mtx(M_TRUE, 'Run 310')
        a = AbifFile(data, 'x.mtx')
        self.assertEqual(a.value('MTRD'), 4)
        inv = mx.invert(M_TRUE)
        self.assertEqual(a.value('MTRX')[0], int(inv[0][0] * 1000))
        self.assertEqual(len(a.value('MRAW')), 16)
        back = mx.mtx_to_matrix(data)
        self.assertEqual(back['comment'], 'Run 310')
        self.assertEqual(back['matrix'], [[mx.round4(v) for v in r] for r in M_TRUE])

    def test_mtx_without_raw_values_uses_inverse(self):
        inv = mx.invert(M_TRUE)
        mtrx = [int(v * 1000) for r in inv for v in r]
        data = mx.write_abif([mx._entry('MTRD', 1, 4, 2, 1, struct.pack('>h', 4)),
                              mx._entry('MTRX', 1, 4, 2, 16, struct.pack('>16h', *mtrx))])
        back = mx.mtx_to_matrix(data)
        self.assertEqual(back['source'], 'MTRX')
        for r in range(4):
            for c in range(4):
                self.assertAlmostEqual(back['matrix'][r][c], M_TRUE[r][c], delta=0.01)

    def test_request_actions(self):
        rng = random.Random(3)
        files = [fsa_310(standard_traces(i, rng)) for i in range(4)]
        r = mx.matrix_request({'action': 'make', 'dyes': 4, 'standards': [
            {'fsa_b64': base64.b64encode(f).decode(), 'filename': f'{i}.fsa', 'start': 200, 'points': 100000} for i, f in enumerate(files)]})
        self.assertNotIn('error', r)
        self.assertAlmostEqual(r['matrix'][1][0], 0.6, delta=0.005)
        r = mx.matrix_request({'action': 'make', 'dyes': 4, 'standards': [{'fsa_b64': base64.b64encode(files[0]).decode()}]})
        self.assertEqual(r['code'], 'no_file')
        self.assertIn('"G"', r['error'])
        self.assertEqual(mx.matrix_request({'action': 'validate', 'matrix': [[1, 3, 0, 0]] * 4})['code'], 'values')
        s = mx.matrix_request({'action': 'sniff', 'fsa_b64': base64.b64encode(files[0]).decode()})
        self.assertEqual((s['is_310'], s['embedded']), (True, False))
        exp = mx.matrix_request({'action': 'export', 'matrix': M_TRUE, 'comment': 'c'})
        imp = mx.matrix_request({'action': 'import', 'mtx_b64': exp['mtx_b64']})
        self.assertEqual(imp['dyes'], 4)


class MultiComponentTests(unittest.TestCase):
    def test_unmixes_and_truncates(self):
        dyes = [[100, 2000, 0], [0, 500, 1000], [10, 0, 0], [0, 0, 300]]
        filters = [[sum(M_TRUE[j][i] * dyes[i][t] for i in range(4)) for t in range(3)] for j in range(4)]
        out = mx.multicomponent(filters, mx.invert(M_TRUE), int16=False)
        for i in range(4):
            for t in range(3):
                self.assertLessEqual(abs(out[i][t] - dyes[i][t]), 1)
        wrapped = mx.multicomponent([[40000], [0], [0], [0]], [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]])
        self.assertEqual(wrapped[0][0], 40000 - 65536)

    def test_analysis_of_310_sample_needs_matrix(self):
        from genemapper_open.server import analyze
        rng = random.Random(4)
        dyes = [dye_trace(rng, 3000) for _ in range(4)]
        filters = [[int(sum(M_TRUE[j][i] * dyes[i][t] for i in range(4))) for t in range(3000)] for j in range(4)]
        data = fsa_310(filters)
        with self.assertRaisesRegex(ValueError, 'MNF'):
            analyze(data, 'a.fsa')
        out = analyze(data, 'a.fsa', matrix=json_dumps(M_TRUE))
        self.assertEqual(out['matrix']['source'], 'library')
        ch1 = next(c for c in out['channels'] if c['tag'] == 'DATA1')
        self.assertTrue(ch1['multicomponented'])
        self.assertLessEqual(max(abs(ch1['values'][t] - int(dyes[0][t])) for t in range(3000)), 2)
        other = fsa_310(filters, model='3130')
        self.assertNotIn('matrix', analyze(other, 'b.fsa'))


def json_dumps(v):
    import json
    return json.dumps(v)


if __name__ == '__main__':
    unittest.main()
