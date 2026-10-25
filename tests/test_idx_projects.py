import base64
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
import urllib.error
import urllib.request

from genemapper_open.idx_projects import ProjectLibrary

META = {'comment': 'run 1', 'run_folders': ['Run 1'],
        'samples': [{'id': 's1', 'file': 'a.fsa', 'name': 'a'}, {'id': 's2', 'file': 'b.fsa', 'name': 'b'}],
        'snapshots': {'methods': [], 'kits': []}}


class LibraryTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.lib = ProjectLibrary(Path(self.tmp.name) / 'projects')

    def tearDown(self):
        self.tmp.cleanup()

    def test_security_group_access(self):
        allowed = {('Casework Security Group', 'read')}
        calls = []

        def access(op, group, owner, check=False):
            calls.append((op, group, owner))
            ok = (group, op) in allowed or group == 'GeneMapper ID-X Security Group'
            if check:
                return ok
            if not ok:
                raise PermissionError(op)
            return True

        self.lib.save_meta('open', META, create=True, access=access, user_name='u1')
        with self.assertRaises(PermissionError):
            self.lib.save_meta('secret', {**META, 'security_group': 'Casework Security Group'}, create=True, access=access, user_name='u1')
        self.lib.save_meta('secret', {**META, 'security_group': 'Casework Security Group'}, create=True, user_name='u2')
        listed = self.lib.list(access)
        self.assertEqual({r['name']: (r['security_group'], r['owner']) for r in listed},
                         {'open': ('GeneMapper ID-X Security Group', 'u1'), 'secret': ('Casework Security Group', 'u2')})
        allowed.clear()
        self.assertEqual([r['name'] for r in self.lib.list(access)], ['open'])
        with self.assertRaises(PermissionError):
            self.lib.open('secret', access)
        with self.assertRaises(PermissionError):
            self.lib.delete('secret', access)
        # owner is kept on later saves by another user
        self.assertEqual(self.lib.save_meta('open', META, access=access, user_name='u3')['owner'], 'u1')

    def test_save_list_open_sample_round_trip(self):
        self.assertEqual(self.lib.list(), [])
        saved = self.lib.save_meta('Проект GF-1', META, create=True, overwrite=False)
        self.assertEqual(saved['name'], 'Проект GF-1')
        fsa = b'ABIF' + bytes(range(256))
        self.lib.save_sample('Проект GF-1', 's1', fsa_b64=base64.b64encode(fsa).decode(), data={'report': {'calls': [1, 2]}})
        rows = self.lib.list()
        self.assertEqual([(r['name'], r['samples'], r['comment']) for r in rows], [('Проект GF-1', 2, 'run 1')])
        meta = self.lib.open('Проект GF-1')
        self.assertEqual([(s['id'], s['has_fsa'], s['has_data']) for s in meta['samples']], [('s1', True, True), ('s2', False, False)])
        sample = self.lib.open_sample('Проект GF-1', 's1')
        self.assertEqual(base64.b64decode(sample['fsa_b64']), fsa)
        self.assertEqual(sample['data'], {'report': {'calls': [1, 2]}})
        self.assertEqual(self.lib.open_sample('Проект GF-1', 's2'), {'id': 's2', 'fsa_b64': None, 'data': None})

    def test_removed_samples_are_pruned_and_names_checked(self):
        self.lib.save_meta('P', META, create=True)
        self.lib.save_sample('P', 's2', data={'x': 1})
        self.lib.save_meta('P', {**META, 'samples': META['samples'][:1]})
        folder = next(p for p in self.lib.root.iterdir())
        self.assertEqual(sorted(p.name for p in (folder / 'samples').iterdir()), [])
        with self.assertRaises(ValueError):
            self.lib.save_meta('P', META, create=True, overwrite=False)
        for bad in ('', ' x', 'a/b', '..', 'a' * 121, 'x:y'):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                self.lib.save_meta(bad, META, create=True)
        for bad_id in ('../x', 'a b', ''):
            with self.subTest(bad_id=bad_id), self.assertRaises(ValueError):
                self.lib.save_sample('P', bad_id, data={})
        with self.assertRaises(ValueError):
            self.lib.save_meta('Q', {'samples': [{'id': '../evil'}]}, create=True)
        with self.assertRaises(ValueError):
            self.lib.open('missing')

    def test_export_import_rename_save_as(self):
        import io
        import zipfile
        meta = {**META, 'samples': [{'id': 's1', 'file': 'a.fsa', 'name': 'a', 'status': 'analyzed'},
                                    {'id': 's2', 'file': 'b.fsa', 'name': 'b', 'status': 'unanalyzed'}],
                'snapshots': {'methods': [{'name': 'M1'}], 'kits': []}}
        self.lib.save_meta('P1', meta, create=True, user_name='u1')
        fsa = b'ABIF' + bytes(200)
        self.lib.save_sample('P1', 's1', fsa_b64=base64.b64encode(fsa).decode(), data={'report': {'calls': [7]}})
        plain = self.lib.request({'action': 'export', 'name': 'P1', 'settings': False})
        z = zipfile.ZipFile(io.BytesIO(base64.b64decode(plain['package_b64'])))
        self.assertEqual(sorted(z.namelist()), ['opentrace-project.json', 'project.json', 'samples/s1.json.gz'])
        self.assertNotIn('snapshots', json.loads(z.read('project.json')))
        full = self.lib.request({'action': 'export', 'name': 'P1', 'settings': True, 'sample_files': True})
        with self.assertRaisesRegex(ValueError, 'already been used'):
            self.lib.request({'action': 'import', 'package_b64': full['package_b64']})
        r = self.lib.request({'action': 'import', 'package_b64': full['package_b64'], 'new_name': 'P2', 'mode': 'overwrite'},
                             user_name='u9')
        self.assertEqual((r['name'], r['samples'], r['sample_files'], r['status_changed'], r['owner']), ('P2', 2, 1, 1, 'u9'))
        self.assertEqual(r['snapshots']['methods'], [{'name': 'M1'}])
        opened = self.lib.open('P2')
        self.assertEqual([s['status'] for s in opened['samples']], ['changed', 'unanalyzed'])
        self.assertEqual(base64.b64decode(self.lib.open_sample('P2', 's1')['fsa_b64']), fsa)
        r = self.lib.request({'action': 'import', 'package_b64': plain['package_b64'], 'new_name': 'P3'})
        self.assertEqual((r['sample_files'], r['status_changed'], r['snapshots']), (0, 0, {}))
        self.assertIsNone(self.lib.open_sample('P3', 's1')['fsa_b64'])
        self.assertEqual(self.lib.open_sample('P3', 's1')['data'], {'report': {'calls': [7]}})
        # hostile archives: not a package, path members ignored
        with self.assertRaisesRegex(ValueError, 'Not an OpenTrace'):
            self.lib.request({'action': 'import', 'package_b64': base64.b64encode(b'PK junk').decode()})
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, 'w') as w:
            w.writestr('opentrace-project.json', json.dumps({'format': 'OpenTrace project export'}))
            w.writestr('project.json', json.dumps({'name': 'Evil', 'samples': [{'id': 's1'}]}))
            w.writestr('../../evil.txt', 'x')
        self.lib.request({'action': 'import', 'package_b64': base64.b64encode(buf.getvalue()).decode()})
        self.assertFalse((Path(self.tmp.name) / 'evil.txt').exists())
        # rename / save as
        self.lib.request({'action': 'rename', 'name': 'P3', 'new_name': 'P3 renamed'})
        self.assertIn('P3 renamed', [p['name'] for p in self.lib.list()])
        with self.assertRaisesRegex(ValueError, 'already been used'):
            self.lib.request({'action': 'rename', 'name': 'P2', 'new_name': 'P1'})
        self.lib.request({'action': 'save_as', 'name': 'P1', 'new_name': 'P1 copy'})
        self.assertEqual(self.lib.open_sample('P1 copy', 's1')['data'], {'report': {'calls': [7]}})

    def test_same_slug_different_names_and_delete(self):
        self.lib.save_meta('a b', META, create=True)
        self.lib.save_meta('a_b', META, create=True)
        self.assertEqual(len(list(self.lib.root.iterdir())), 2)
        self.lib.delete('a b')
        self.assertEqual([r['name'] for r in self.lib.list()], ['a_b'])
        self.assertEqual(self.lib.request({'action': 'list'})['projects'][0]['name'], 'a_b')
        with self.assertRaises(ValueError):
            self.lib.request({'action': 'nope'})


class HttpTests(unittest.TestCase):
    def test_route_uses_projects_dir(self):
        with tempfile.TemporaryDirectory() as tmp:
            proc = subprocess.Popen([sys.executable, '-u', '-m', 'genemapper_open', 'serve', '--port', '0',
                                     '--projects-dir', tmp], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
            try:
                base = re.search(r'http://127\.0\.0\.1:\d+/', proc.stdout.readline()).group(0)
                token = re.search(r"const TOKEN='([^']+)'", urllib.request.urlopen(base, timeout=5).read().decode()).group(1)
                def post(payload, expected=200):
                    req = urllib.request.Request(base + 'api/idx-project', data=json.dumps(payload).encode(),
                        headers={'X-Session-Token': token, 'Content-Type': 'application/json'}, method='POST')
                    try: resp = urllib.request.urlopen(req, timeout=10)
                    except urllib.error.HTTPError as e: resp = e
                    with resp:
                        self.assertEqual(resp.code, expected); return json.load(resp)
                post({'action': 'save_meta', 'name': 'HTTP', 'project': META, 'create': True})
                listing = post({'action': 'list'})
                self.assertEqual(listing['root'], tmp)
                self.assertEqual(listing['projects'][0]['name'], 'HTTP')
                self.assertIn('error', post({'action': 'open', 'name': 'missing'}, 400))
            finally:
                proc.terminate(); proc.wait(timeout=5); proc.stdout.close()


if __name__ == '__main__':
    unittest.main()
