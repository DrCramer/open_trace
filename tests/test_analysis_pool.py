import os
import unittest

from genemapper_open import server as S
from genemapper_open.abif import AbifError
from genemapper_open.demo import demo_bytes


class AnalysisPool(unittest.TestCase):
    def setUp(self):
        self._old = os.environ.get('OPENTRACE_WORKERS')

    def tearDown(self):
        S.shutdown_pool()
        if self._old is None:
            os.environ.pop('OPENTRACE_WORKERS', None)
        else:
            os.environ['OPENTRACE_WORKERS'] = self._old

    def test_worker_count_env(self):
        os.environ['OPENTRACE_WORKERS'] = '3'
        self.assertEqual(S.worker_count(), 3)
        os.environ['OPENTRACE_WORKERS'] = '1'
        self.assertEqual(S.worker_count(), 1)
        os.environ['OPENTRACE_WORKERS'] = 'x'
        self.assertEqual(S.worker_count(), 1)
        os.environ['OPENTRACE_WORKERS'] = '999'
        self.assertEqual(S.worker_count(), 16)

    def test_pool_result_equals_direct_call(self):
        raw = demo_bytes()
        direct = S.analyze(raw, 'd.fsa', 100.0, 8, '')
        os.environ['OPENTRACE_WORKERS'] = '2'
        pooled = S.analyze_dispatch(raw, 'd.fsa', 100.0, 8, '')
        if isinstance(pooled, bytes):
            pooled = __import__('json').loads(pooled)
        self.assertEqual(pooled, direct)

    def test_errors_keep_their_type(self):
        os.environ['OPENTRACE_WORKERS'] = '2'
        with self.assertRaises(AbifError):
            S.analyze_dispatch(b'not an abif file', 'bad.fsa', 100.0, 8, '')

    def test_single_worker_runs_in_process(self):
        os.environ['OPENTRACE_WORKERS'] = '1'
        self.assertIsNone(S._analysis_pool())


if __name__ == '__main__':
    unittest.main()
