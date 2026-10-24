"""Project library for the desktop-style workspace (``/idx/``).

A project is a folder ``<root>/<slug>/`` with ``project.json`` (name, dates,
run folders, per-sample settings, snapshots of the analysis methods, table
setting and kit texts it uses) and ``samples/<id>.fsa`` (the original bytes)
plus ``samples/<id>.json.gz`` (analysis, compact analysis and STR report).
Writes go to a temporary file first and are then renamed, so an interrupted
save never leaves a half-written file in place.
"""
import base64
import gzip
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import time

SCHEMA = 1
MAX_NAME = 120
MAX_META_BYTES = 8 * 1024 * 1024
MAX_SAMPLE_BYTES = 128 * 1024 * 1024
MAX_FSA_BYTES = 64 * 1024 * 1024
SAMPLE_ID = re.compile(r'^[A-Za-z0-9_-]{1,64}$')
NAME = re.compile(r'^[^\x00-\x1f<>:"/\\|?*]{1,120}$')


def default_root():
    env = os.environ.get('OPENTRACE_PROJECTS')
    return Path(env) if env else Path.home() / 'OpenTrace' / 'projects'


def _check_name(name):
    if not isinstance(name, str) or not NAME.match(name) or name != name.strip() or name in ('.', '..'):
        raise ValueError('Project name: 1..120 characters without control characters or < > : " / \\ | ? *')
    return name


def _slug(name):
    base = re.sub(r'[^A-Za-z0-9_-]+', '_', name).strip('_')[:40] or 'project'
    return base


def _atomic_write(path, data):
    tmp = path.with_name(path.name + '.tmp' + secrets.token_hex(4))
    with open(tmp, 'wb') as handle:
        handle.write(data)
    os.replace(tmp, path)


class ProjectLibrary:
    def __init__(self, root=None):
        self.root = Path(root) if root is not None else default_root()

    def _dirs(self):
        if not self.root.is_dir():
            return []
        return [d for d in self.root.iterdir() if d.is_dir() and (d / 'project.json').is_file()]

    def _read_meta(self, folder):
        raw = (folder / 'project.json').read_bytes()
        if len(raw) > MAX_META_BYTES:
            raise ValueError('project.json exceeds 8 MiB')
        meta = json.loads(raw)
        if not isinstance(meta, dict) or meta.get('schema') != SCHEMA or not isinstance(meta.get('name'), str):
            raise ValueError(f'{folder.name}: not an OpenTrace project')
        return meta

    def _find(self, name):
        _check_name(name)
        for folder in self._dirs():
            try:
                if self._read_meta(folder)['name'] == name:
                    return folder
            except (ValueError, OSError):
                continue
        return None

    def list(self, access=None):
        out = []
        for folder in self._dirs():
            try:
                meta = self._read_meta(folder)
            except (ValueError, OSError):
                continue
            # Open Project lists only projects whose security group the logged-in user may read
            if access and not access('read', meta.get('security_group'), meta.get('owner'), check=True):
                continue
            out.append({'name': meta['name'], 'created': meta.get('created'), 'modified': meta.get('modified'),
                        'samples': len(meta.get('samples', [])), 'comment': meta.get('comment', ''),
                        'security_group': meta.get('security_group') or 'GeneMapper ID-X Security Group',
                        'owner': meta.get('owner', '')})
        return sorted(out, key=lambda r: r['name'].lower())

    def save_meta(self, name, project, *, create=False, overwrite=True, access=None, user_name=None):
        _check_name(name)
        if not isinstance(project, dict) or not isinstance(project.get('samples'), list):
            raise ValueError('Project with a samples list required')
        for sample in project['samples']:
            if not isinstance(sample, dict) or not SAMPLE_ID.match(str(sample.get('id', ''))):
                raise ValueError('Each sample needs an id of letters, digits, _ or -')
        folder = self._find(name)
        if folder is not None and create and not overwrite:
            raise ValueError(f'{name} has already been used, please enter a new name.')
        now = time.strftime('%Y-%m-%dT%H:%M:%S')
        group = project.get('security_group') or 'GeneMapper ID-X Security Group'
        if folder is not None:
            old = self._read_meta(folder)
            owner = old.get('owner') or user_name or ''
            if access:
                access('update', old.get('security_group'), old.get('owner'))
                if group != (old.get('security_group') or 'GeneMapper ID-X Security Group'):
                    access('create', group, None)
        else:
            owner = user_name or ''
            if access:
                access('create', group, None)
        if folder is None:
            self.root.mkdir(parents=True, exist_ok=True)
            slug, k = _slug(name), 1
            folder = self.root / slug
            while folder.exists():
                k += 1
                folder = self.root / f'{slug}_{k}'
            (folder / 'samples').mkdir(parents=True)
            created = now
        else:
            created = self._read_meta(folder).get('created', now)
        meta = {**project, 'schema': SCHEMA, 'name': name, 'created': created, 'modified': now, 'security_group': group,
                'owner': owner}
        data = json.dumps(meta, ensure_ascii=False, allow_nan=False).encode('utf-8')
        if len(data) > MAX_META_BYTES:
            raise ValueError('Project metadata exceeds 8 MiB')
        _atomic_write(folder / 'project.json', data)
        keep = {str(s['id']) for s in project['samples']}
        for path in (folder / 'samples').iterdir():
            stem = path.name.split('.', 1)[0]
            if stem not in keep and not path.name.endswith('.tmp'):
                path.unlink()
        return {'name': name, 'created': created, 'modified': now, 'security_group': group, 'owner': owner}

    def _guard(self, folder, access, op):
        if access:
            meta = self._read_meta(folder)
            access(op, meta.get('security_group'), meta.get('owner'))

    def save_sample(self, name, sample_id, *, fsa_b64=None, data=None, access=None):
        folder = self._find(name)
        if folder is None:
            raise ValueError(f'{name} does not exist.')
        self._guard(folder, access, 'update')
        if not SAMPLE_ID.match(str(sample_id)):
            raise ValueError('Invalid sample id')
        samples = folder / 'samples'
        samples.mkdir(exist_ok=True)
        if fsa_b64 is not None:
            raw = base64.b64decode(fsa_b64, validate=True)
            if len(raw) > MAX_FSA_BYTES:
                raise ValueError('FSA exceeds 64 MiB')
            _atomic_write(samples / f'{sample_id}.fsa', raw)
        if data is not None:
            encoded = json.dumps(data, ensure_ascii=False, allow_nan=False).encode('utf-8')
            if len(encoded) > MAX_SAMPLE_BYTES:
                raise ValueError('Sample data exceeds 128 MiB')
            _atomic_write(samples / f'{sample_id}.json.gz', gzip.compress(encoded, 6))
        return {'id': sample_id}

    def open(self, name, access=None):
        folder = self._find(name)
        if folder is None:
            raise ValueError(f'{name} does not exist.')
        self._guard(folder, access, 'read')
        meta = self._read_meta(folder)
        stored = {p.name for p in (folder / 'samples').iterdir()} if (folder / 'samples').is_dir() else set()
        for sample in meta.get('samples', []):
            sid = str(sample['id'])
            sample['has_fsa'] = f'{sid}.fsa' in stored
            sample['has_data'] = f'{sid}.json.gz' in stored
        return meta

    def open_sample(self, name, sample_id, access=None):
        folder = self._find(name)
        if folder is None:
            raise ValueError(f'{name} does not exist.')
        self._guard(folder, access, 'read')
        if not SAMPLE_ID.match(str(sample_id)):
            raise ValueError('Invalid sample id')
        out = {'id': sample_id, 'fsa_b64': None, 'data': None}
        fsa = folder / 'samples' / f'{sample_id}.fsa'
        if fsa.is_file():
            out['fsa_b64'] = base64.b64encode(fsa.read_bytes()).decode('ascii')
        blob = folder / 'samples' / f'{sample_id}.json.gz'
        if blob.is_file():
            raw = gzip.decompress(blob.read_bytes())
            if len(raw) > MAX_SAMPLE_BYTES:
                raise ValueError('Sample data exceeds 128 MiB')
            out['data'] = json.loads(raw)
        return out

    def delete(self, name, access=None):
        folder = self._find(name)
        if folder is None:
            raise ValueError(f'{name} does not exist.')
        self._guard(folder, access, 'delete')
        shutil.rmtree(folder)
        return {'deleted': name}

    def request(self, request, access=None, user_name=None):
        """access(op, security_group, owner, check=False) raises (or, with check=True, returns False) when the
        current user's user groups lack that right in the project's security group."""
        if not isinstance(request, dict):
            raise ValueError('Project request must be an object')
        action = request.get('action')
        if action == 'list':
            return {'root': str(self.root), 'projects': self.list(access)}
        if action == 'save_meta':
            return self.save_meta(request.get('name'), request.get('project'), create=bool(request.get('create')),
                                  overwrite=bool(request.get('overwrite', True)), access=access, user_name=user_name)
        if action == 'save_sample':
            return self.save_sample(request.get('name'), request.get('id'), fsa_b64=request.get('fsa_b64'),
                                    data=request.get('data'), access=access)
        if action == 'open':
            return self.open(request.get('name'), access)
        if action == 'open_sample':
            return self.open_sample(request.get('name'), request.get('id'), access)
        if action == 'delete':
            return self.delete(request.get('name'), access)
        raise ValueError('action must be list, save_meta, save_sample, open, open_sample or delete')
