"""Users, access rights, audit trail and electronic signatures (GeneMapper ID-X Admin menu).

The default model mirrors what GeneMapper ID-X 1.6 ships in ``service/bin/DefaultGMSecurity.acc``
(access-control tables), ``piconfig/auditMap.db.xml`` (audit maps) and ``piconfig/esigGM.db.xml``
(e-signature action groups).  Original password hashes are not reused: shipped accounts start without
a password and cannot log in until an administrator sets one.

Storage: one JSON document (``security.json``) plus an append-only JSON-lines audit file whose records
carry an HMAC-SHA256 over their content and the previous record's code (``AUDIT_RECORD_HASH_CODE``).
"""
from __future__ import annotations

import copy
import datetime as _dt
import hashlib
import hmac
import json
import os
import re
import secrets
import threading
import xml.etree.ElementTree as ET
from pathlib import Path

# ---------------------------------------------------------------- catalogue of rights
GM = 'GeneMapper ID-X'
ACC = 'AdminToolAcc'
AUDIT = 'AuditGUI'
ESIG = 'ESig'
CATALOG = [
    (GM, 'Project', ['READ_PROJECT', 'UPDATE_PROJECT']),
    (GM, 'Analysis Method', ['READ_ANALYSIS_METHOD', 'UPDATE_ANALYSIS_METHOD', 'UPDATE_ANALYSIS_RANGE',
                             'UPDATE_SIZING_RANGE', 'SAVE_AS_ANALYSIS_METHOD']),
    (GM, 'Panel Manager', ['READ_PANEL', 'UPDATE_PANEL']),
    (GM, 'Size Standard', ['READ_SIZE_STANDARD', 'UPDATE_SIZE_STANDARD']),
    (GM, 'Matrix', ['READ_MATRIX', 'UPDATE_MATRIX']),
    (GM, 'Profile Manager', ['READ_PROFILE_CONCORDANCE', 'UPDATE_PROFILE_CONCORDANCE']),
    (GM, 'Administration', ['Remote Shutdown']),
    (ACC, 'General', ['May run application', 'Add User to User Group', 'Associate User Groups with Security Groups',
                      'Associate User Groups with Users', 'Change Password Policies', 'Import Security Data',
                      'Export Security Data']),
    (ACC, 'User Groups', ['Create', 'Modify', 'Change Control Security Group', 'Delete']),
    (ACC, 'Profiles', ['Create', 'Modify', 'Change Control Security Group', 'Delete']),
    (ACC, 'Users', ['Create', 'Modify', 'Change Control Security Group', 'Delete']),
    (ACC, 'Security Groups', ['Create', 'Modify', 'Change Control Security Group']),
    (ESIG, 'ESig Administration Tool', ['May run application']),
    (AUDIT, 'Audit Management', ['Backup Audit Records', 'Restore Audit Records', 'Delete Audit Records']),
    (AUDIT, 'Audit History Viewer', ['May run application']),
    (AUDIT, 'Map Configuration Tool', ['May run application', 'Enable and disable maps', 'Change auditing state']),
]
FUNCTIONS = [f'{p}/{m}/{f}' for p, m, fs in CATALOG for f in fs]
PRODUCTS = list(dict.fromkeys(p for p, _, _ in CATALOG))
MODULES = list(dict.fromkeys(f'{p}/{m}' for p, m, _ in CATALOG))
_SHORT = {}
for _key in FUNCTIONS:
    _SHORT.setdefault(_key.rsplit('/', 1)[1], _key)  # GeneMapper function names are unique


def function_key(name):
    """Accepts a full ``product/module/function`` key or a unique GeneMapper function name."""
    if name in FUNCTIONS:
        return name
    if name in _SHORT and _SHORT[name].startswith(GM + '/'):
        return _SHORT[name]
    raise ValueError(f'Unknown function: {name}')


# ---------------------------------------------------------------- shipped defaults (DefaultGMSecurity.acc)
def _profile(name, description, default, rights):
    return {'name': name, 'description': description, 'default_enable': default, 'rights': rights}


DEFAULT_PROFILES = [
    _profile('Admin Profile', 'Admin Profile for Access Control', True, {}),
    _profile('Scientist', 'Full GeneMapper ID-X Control. No access to Security, Audit and ESig configuration tools.',
             False, {GM: True, f'{GM}/Administration/Remote Shutdown': False}),
    _profile('Technician/Read Only', 'This profile has read only access to GeneMapper ID-X. No access to Security, '
             'Audit and ESig configuration tools.', False,
             {f'{GM}/{m}/{f}': True for m, f in [('Analysis Method', 'READ_ANALYSIS_METHOD'), ('Size Standard', 'READ_SIZE_STANDARD'),
                                                 ('Panel Manager', 'READ_PANEL'), ('Project', 'READ_PROJECT'),
                                                 ('Matrix', 'READ_MATRIX'), ('Profile Manager', 'READ_PROFILE_CONCORDANCE')]}),
    _profile('Analyst', 'Access to all GeneMapper ID-X functionality except Analysist Method. Analyst can only edit the '
             'sizing range and analysis range for an Analysis Method. No access to Security, Audit and ESig '
             'configuration tools.', False,
             {GM: True, f'{GM}/Analysis Method': False, f'{GM}/Analysis Method/READ_ANALYSIS_METHOD': True,
              f'{GM}/Analysis Method/UPDATE_ANALYSIS_RANGE': True, f'{GM}/Analysis Method/UPDATE_SIZING_RANGE': True,
              f'{GM}/Analysis Method/SAVE_AS_ANALYSIS_METHOD': True,
              f'{GM}/Profile Manager/UPDATE_PROFILE_CONCORDANCE': False, f'{GM}/Administration/Remote Shutdown': False}),
]
_U = [  # user name, full name, description, profile, user groups
    ('Administrator', 'Administrator user', 'User Administrator', 'Admin Profile', ['Administrators', 'Casework User Group', 'Databasing User Group']),
    ('gmidx', 'GeneMapper DefaultUser', 'GeneMapper DefaultUser', 'Admin Profile', ['Administrators', 'Casework User Group', 'Databasing User Group']),
    ('Casework Analyst', 'Casework Analyst', '', 'Analyst', ['Casework User Group']),
    ('Databasing Analyst', 'Databasing Analyst', '', 'Analyst', ['Databasing User Group']),
    ('Casework Tech', 'Casework Technician', '', 'Technician/Read Only', ['Casework User Group']),
    ('Databasing Tech', 'Databasing Technician', '', 'Technician/Read Only', ['Databasing User Group']),
    ('Casework Tech Lead', 'Casework Technical Lead', '', 'Admin Profile', ['Casework User Group']),
    ('Databasing Tech Lead', 'Databasing Technical Lead', '', 'Admin Profile', ['Databasing User Group']),
    ('Lab Director', 'Laboratory Director', '', 'Admin Profile', ['Administrators', 'Casework User Group', 'Databasing User Group']),
    ('Practice User', 'Practice User', 'For use with Getting Started Guide only', 'Scientist', ['Practice User Group']),
]
DEFAULT_USER_GROUPS = [
    {'name': 'Administrators', 'description': 'Administrators for Access Control', 'all_users': False},
    {'name': 'All Users', 'description': 'All Users for Access Control', 'all_users': True},
    {'name': 'Casework User Group', 'description': '', 'all_users': False},
    {'name': 'Databasing User Group', 'description': '', 'all_users': False},
    {'name': 'Practice User Group', 'description': 'For use with Getting Started Guide only', 'all_users': False},
]
DEFAULT_SECURITY_GROUP = 'GeneMapper ID-X Security Group'
RIGHT_NAMES = ['read', 'update', 'create', 'delete', 'control', 'owner_update', 'owner_delete']
_FULL = dict.fromkeys(RIGHT_NAMES, True)
_RU = {'read': True, 'update': True, 'create': False, 'delete': False, 'control': False, 'owner_update': False, 'owner_delete': False}
DEFAULT_SECURITY_GROUPS = [
    {'name': 'Admin Security Group', 'description': 'Admin Security Group for Access Control',
     'rights': {'Administrators': _FULL}},
    {'name': 'GeneMapper ID-X Security Group', 'description': 'GeneMapper default security group',
     'rights': {'All Users': _FULL, 'Administrators': _FULL, 'Casework User Group': _RU, 'Databasing User Group': _RU}},
    {'name': 'Casework Security Group', 'description': '', 'rights': {'Administrators': _FULL, 'Casework User Group': _RU}},
    {'name': 'Databasing Security Group', 'description': '', 'rights': {'Administrators': _FULL, 'Databasing User Group': _RU}},
    {'name': 'Practice Security Group', 'description': 'For use with Getting Started Guide only',
     'rights': {'Administrators': _FULL, 'Practice User Group': _RU,
                'All Users': {**_RU, 'owner_update': True, 'owner_delete': True}}},
]
# AUTH_POLICIES row of DefaultGMSecurity.acc
DEFAULT_POLICIES = {'max_failed_attempts': 5, 'send_log_message': True, 'upon_failure': 'suspend', 'suspension_minutes': 10,
                    'lifetime_enabled': False, 'lifetime_days': 90, 'grace_logins': 6, 'reuse_restricted': True,
                    'passwords_kept': 10, 'min_length': 6, 'reuse_period_days': 30}
UPON_FAILURE = ('remain_active', 'disable', 'suspend')

# auditMap.db.xml: AUDIT_STATE 1 = On, 2 = Silent, 0 = Off; ENABLED = auditable
_A = lambda name, state, auditable=True, typ='primitive', form=0: {'name': name, 'type': typ, 'state': state,
                                                                   'auditable': auditable, 'form': form}
_CMD = lambda st='silent': [_A('created', st), _A('modified', st), _A('deleted', st), _A('name', st, False)]
DEFAULT_AUDIT_MAPS = [
    {'name': 'Allele', 'type': 'AlleleT', 'enabled': True, 'attributes': _CMD('on')[:3] + [_A('name', 'silent', False)]},
    {'name': 'Kit', 'type': 'KitT', 'enabled': True, 'attributes': _CMD() + [_A('PANEL', 'on', typ='PanelT', form=2), _A('BINSET', 'on', typ='BinSetT', form=2)]},
    {'name': 'SizeStandard', 'type': 'SizeStandardT', 'enabled': True, 'attributes': _CMD()[:3]},
    {'name': 'Sample', 'type': 'SampleT', 'enabled': True, 'attributes': _CMD() + [
        _A('OverrideSQ', 'silent'), _A('OverrideGQ', 'silent'), _A('OverrideCGQ', 'silent'), _A('ALLELE', 'on', typ='AlleleT', form=2)]},
    {'name': 'Marker', 'type': 'MarkerT', 'enabled': True, 'attributes': _CMD() + [_A('BIN', 'on', typ='BinT', form=2)]},
    {'name': 'Run', 'type': 'RunT', 'enabled': True, 'attributes': [
        _A('created', 'silent'), _A('modified', 'silent', False), _A('deleted', 'silent'), _A('name', 'silent', False),
        _A('OverrideIQC', 'silent', False), _A('OverrideSQC', 'silent', False), _A('SAMPLE', 'on', typ='SampleT', form=2)]},
    {'name': 'Panel', 'type': 'PanelT', 'enabled': True, 'attributes': _CMD() + [_A('MARKER', 'on', typ='MarkerT', form=2)]},
    {'name': 'AnalysisMethod', 'type': 'AnalysisMethodT', 'enabled': True, 'attributes': _CMD()},
    {'name': 'BinSet', 'type': 'BinSetT', 'enabled': True, 'attributes': [
        _A('created', 'silent'), _A('modified', 'silent', False), _A('deleted', 'silent'), _A('name', 'silent', False)]},
    {'name': 'Bin', 'type': 'BinT', 'enabled': True, 'attributes': _CMD()},
    {'name': 'Project', 'type': 'ProjectT', 'enabled': True, 'attributes': _CMD() + [_A('analyzed', 'silent'), _A('RUN', 'on', typ='RunT', form=2)]},
    {'name': 'Matrix', 'type': 'MatrixT', 'enabled': True, 'attributes': _CMD()[:3]},
]
AUDIT_STATES = ('on', 'off', 'silent')
_CHALLENGE = 'By entering your UserID and Password, you are signing the action'
_EG = lambda name, noun, title: {'name': name, 'required': False, 'challenge': _CHALLENGE, 'actions': [
    {'name': f'CREATE_{noun}', 'meaning': f'Create New {title}'}, {'name': f'MODIFY_{noun}', 'meaning': f'Modify {title}'},
    {'name': f'DELETE_{noun}', 'meaning': f'Delete {title}'}]}
DEFAULT_ESIG = [_EG('GM_PROJECT', 'PROJECT', 'Project'), _EG('GM_CHEMISTRY_KIT', 'CHEMISTRY_KIT', 'Chemistry Kit'),
                _EG('GM_ANALYSIS_METHOD', 'ANALYSIS_METHOD', 'Analysis Method'), _EG('GM_SIZE_STANDARD', 'SIZE_STANDARD', 'Size Standard'),
                _EG('GM_MATRIX', 'MATRIX', 'Matrix')]

NAME_RE = re.compile(r'^[A-Za-z0-9][A-Za-z0-9 ._-]{0,29}$')
ITERATIONS = 120_000


def _now():
    return _dt.datetime.now(_dt.timezone.utc)


def _iso(t):
    return t.strftime('%Y-%m-%dT%H:%M:%SZ')


def _parse(s):
    return _dt.datetime.strptime(s, '%Y-%m-%dT%H:%M:%SZ').replace(tzinfo=_dt.timezone.utc)


def _hash_password(password, salt=None, iterations=None):
    salt = salt or secrets.token_hex(16)
    iterations = iterations or ITERATIONS
    digest = hashlib.pbkdf2_hmac('sha256', password.encode('utf-8'), bytes.fromhex(salt), iterations).hex()
    return {'salt': salt, 'iterations': iterations, 'hash': digest}


def _check_password(password, record):
    if not record:
        return False
    test = _hash_password(password, record['salt'], record['iterations'])
    return hmac.compare_digest(test['hash'], record['hash'])


def default_document():
    users = [{'name': n, 'full_name': f, 'description': d, 'profile': p, 'groups': g, 'disabled': False,
              'password': None, 'password_created': None, 'history': [], 'failed': [], 'suspended_until': None,
              'grace_used': 0, 'created': None, 'last_login': None} for n, f, d, p, g in _U]
    return {'schema': 1, 'settings': {'require_login': False}, 'policies': dict(DEFAULT_POLICIES),
            'profiles': copy.deepcopy(DEFAULT_PROFILES), 'users': users, 'user_groups': copy.deepcopy(DEFAULT_USER_GROUPS),
            'security_groups': copy.deepcopy(DEFAULT_SECURITY_GROUPS),
            'audit': {'enabled': True, 'maps': copy.deepcopy(DEFAULT_AUDIT_MAPS), 'reasons': [], 'free_text': True},
            'esig': copy.deepcopy(DEFAULT_ESIG)}


def profile_allows(profile, function):
    """Most specific entry wins: function, then module, then product, then the profile default."""
    key = function_key(function)
    rights = profile.get('rights', {})
    product, module, _ = key.split('/', 2)
    for k in (key, f'{product}/{module}', product):
        if k in rights:
            return bool(rights[k])
    return bool(profile.get('default_enable'))


class AccessDenied(ValueError):
    pass


class SecurityStore:
    """Thread-safe security document + audit trail kept in ``root``."""

    def __init__(self, root):
        self.root = Path(root).expanduser()
        self.root.mkdir(parents=True, exist_ok=True)
        self.doc_path = self.root / 'security.json'
        self.audit_path = self.root / 'audit.jsonl'
        self.key_path = self.root / 'audit.key'
        self.lock = threading.RLock()
        self.tickets = {}
        if not self.key_path.exists():
            self.key_path.write_text(secrets.token_hex(32), encoding='ascii')
        self.key = bytes.fromhex(self.key_path.read_text(encoding='ascii').strip())
        self.doc = self._load()

    # -------------------------------------------------------- persistence
    def _load(self):
        if self.doc_path.exists():
            doc = json.loads(self.doc_path.read_text(encoding='utf-8'))
            if doc.get('schema') != 1:
                raise ValueError('Unsupported security document')
            base = default_document()
            for k, v in base.items():  # forward-fill new sections
                doc.setdefault(k, v)
            return doc
        doc = default_document()
        stamp = _iso(_now())
        for u in doc['users']:
            u['created'] = stamp
        self._write(doc)
        return doc

    def _write(self, doc=None):
        tmp = self.doc_path.with_suffix('.tmp')
        tmp.write_text(json.dumps(doc or self.doc, ensure_ascii=False, indent=1), encoding='utf-8')
        os.replace(tmp, self.doc_path)

    # -------------------------------------------------------- lookups
    def user(self, name):
        return next((u for u in self.doc['users'] if u['name'] == name), None)

    def profile(self, name):
        return next((p for p in self.doc['profiles'] if p['name'] == name), None)

    def can(self, user, function):
        if user is None:
            return False
        prof = self.profile(user['profile'])
        return bool(prof) and profile_allows(prof, function)

    def rights(self, user):
        return {k: self.can(user, k) for k in FUNCTIONS}

    def require(self, user, *functions):
        if not any(self.can(user, f) for f in functions):
            raise AccessDenied('You do not have sufficient rights to perform this action.')

    def session_user(self, ticket):
        """User bound to a login ticket; without required login the Administrator account acts locally."""
        with self.lock:
            name = self.tickets.get(ticket or '')
            if name:
                u = self.user(name)
                if u and not u['disabled']:
                    return u
            if not self.doc['settings']['require_login']:
                return self.user('Administrator') or next((u for u in self.doc['users'] if self.can(u, f'{ACC}/General/May run application')), None)
            return None

    def groups_of(self, user):
        return [g['name'] for g in self.doc['user_groups'] if g.get('all_users') or g['name'] in user.get('groups', [])]

    # -------------------------------------------------------- data-item access (security groups)
    def object_rights(self, user, group):
        """Rights of the user's user groups in one security group (union over the groups the user belongs to).
        An item whose security group no longer exists falls back to the default GeneMapper ID-X group."""
        if user is None:
            return set()
        sg = next((g for g in self.doc['security_groups'] if g['name'] == group), None)
        if sg is None:
            sg = next((g for g in self.doc['security_groups'] if g['name'] == DEFAULT_SECURITY_GROUP), None)
        if sg is None:
            return set()
        mine = set(self.groups_of(user))
        return {r for ug, rights in sg.get('rights', {}).items() if ug in mine for r, on in rights.items() if on}

    def object_allows(self, user, group, op, owner=None):
        """op: read | create | update | delete | control. Owners may update/delete with owner_update/owner_delete."""
        rights = self.object_rights(user, group)
        if op in rights:
            return True
        if op in ('update', 'delete') and owner and user and owner == user['name'] and f'owner_{op}' in rights:
            return True
        return False

    def require_object(self, user, group, op, owner=None):
        if not self.object_allows(user, group, op, owner):
            raise AccessDenied(f'Access to an item of security group "{group or DEFAULT_SECURITY_GROUP}" ({op}) is not allowed '
                               'for your user groups.')

    # -------------------------------------------------------- authentication
    def _locked(self, u, now):
        return bool(u.get('suspended_until')) and _parse(u['suspended_until']) > now

    def _password_problem(self, u, password):
        pol = self.doc['policies']
        if not isinstance(password, str) or len(password) < pol['min_length'] or len(password) > 30:
            return f"Password must be {pol['min_length']} to 30 characters."
        if any(c.isspace() for c in password):
            return 'Password must not contain spaces.'
        if pol['reuse_restricted']:
            # the current password, the last N kept passwords and any password used within the reuse period
            now = _now()
            history = u.get('history', [])
            kept = history[-pol['passwords_kept']:] if pol['passwords_kept'] else []
            recent = [h for h in history if h.get('at') and (now - _parse(h['at'])).days < pol['reuse_period_days']]
            current = [u['password']] if u.get('password') else []
            if any(_check_password(password, h) for h in current + kept + recent):
                return 'This password was used earlier - please choose another password.'
        return None

    def _set_password(self, u, password):
        problem = self._password_problem(u, password)
        if problem:
            raise ValueError(problem)
        if u.get('password'):
            u.setdefault('history', []).append({**u['password'], 'at': u.get('password_created')})
            u['history'] = u['history'][-max(self.doc['policies']['passwords_kept'], 1):]
        u['password'] = _hash_password(password)
        u['password_created'] = _iso(_now())
        u['grace_used'] = 0

    def login(self, name, password):
        with self.lock:
            now = _now()
            u = self.user(name)
            pol = self.doc['policies']
            if u is None:
                raise ValueError('The username does not exist. Contact the administrator.')
            if u['disabled']:
                raise ValueError('The username is currently disabled, Contact the administrator.')
            if self._locked(u, now):
                left = max(1, int((_parse(u['suspended_until']) - now).total_seconds() // 60) + 1)
                raise ValueError(f'User Name: {name} is currently locked out for {left} minutes.')
            if not u.get('password'):
                raise ValueError('No password has been set for this account. Contact the administrator.')
            if not _check_password(password, u['password']):
                u['failed'] = [t for t in u.get('failed', []) if (now - _parse(t)).total_seconds() < 86400] + [_iso(now)]
                if len(u['failed']) >= pol['max_failed_attempts']:
                    if pol['upon_failure'] == 'disable':
                        u['disabled'] = True
                    elif pol['upon_failure'] == 'suspend':
                        u['suspended_until'] = _iso(now + _dt.timedelta(minutes=pol['suspension_minutes']))
                    u['failed'] = []
                self._write()
                if pol.get('send_log_message'):
                    self._append_unlocked({'map': 'Security', 'instance': name, 'attribute': 'login', 'old': '',
                                           'new': 'failed', 'reason': '', 'event': 'Login failed'}, None)
                raise ValueError('The password is not correct. Re-enter your password or contact the administrator.')
            must_change = False
            if pol['lifetime_enabled'] and u.get('password_created'):
                if (now - _parse(u['password_created'])).days >= pol['lifetime_days']:
                    u['grace_used'] = u.get('grace_used', 0) + 1
                    must_change = u['grace_used'] > pol['grace_logins']
            u['failed'] = []
            u['suspended_until'] = None
            u['last_login'] = _iso(now)
            self._write()
            ticket = secrets.token_urlsafe(24)
            if not must_change:
                self.tickets[ticket] = name
            self._append_unlocked({'map': 'Security', 'instance': name, 'attribute': 'login', 'old': '', 'new': 'success',
                                   'reason': '', 'event': 'Login'}, u)
            return {'ticket': None if must_change else ticket, 'must_change_password': must_change}

    def logout(self, ticket):
        with self.lock:
            name = self.tickets.pop(ticket or '', None)
            if name:
                self._append_unlocked({'map': 'Security', 'instance': name, 'attribute': 'logout', 'old': '', 'new': '',
                                       'reason': '', 'event': 'Logout'}, self.user(name))
            return {'ok': True}

    def change_password(self, name, old, new):
        with self.lock:
            u = self.user(name)
            if u is None or not u.get('password') or not _check_password(old or '', u['password']):
                raise ValueError('Incorrect Old Password.  Please re-enter!')
            if old == new:
                raise ValueError('New password must be different from current password!')
            self._set_password(u, new)
            self._write()
            return {'ok': True}

    def verify_signature(self, user, name, password, group, action):
        """Electronic signature: only the logged-in user may sign; records the signed action."""
        with self.lock:
            if user is None or name != user['name']:
                raise ValueError('Invalid userID.  Only the logged-in user may sign.')
            if not user.get('password') or not _check_password(password or '', user['password']):
                raise ValueError('The password is not correct. Re-enter your password or contact the administrator.')
            g = next((g for g in self.doc['esig'] if g['name'] == group), None)
            a = next((a for a in (g or {}).get('actions', []) if a['name'] == action), None)
            if a is None:
                raise ValueError('Unknown e-signature action')
            rec = self._append_unlocked({'map': 'ESig', 'instance': group, 'attribute': action, 'old': '',
                                         'new': a['meaning'], 'reason': g['challenge'], 'event': 'Electronic Signature'}, user)
            return {'ok': True, 'meaning': a['meaning'], 'record': rec['id']}

    # -------------------------------------------------------- administration
    def public_user(self, u):
        out = {k: u.get(k) for k in ('name', 'full_name', 'description', 'profile', 'groups', 'disabled', 'created', 'last_login',
                                     'password_created')}
        now = _now()
        out['has_password'] = bool(u.get('password'))
        out['locked_out'] = self._locked(u, now)
        out['status'] = 'disabled' if u['disabled'] else 'suspended' if out['locked_out'] else 'active'
        return out

    def admin_state(self):
        d = self.doc
        return {'settings': d['settings'], 'policies': d['policies'], 'profiles': d['profiles'],
                'users': [self.public_user(u) for u in d['users']], 'user_groups': [
                    {**g, 'members': [u['name'] for u in d['users'] if g.get('all_users') or g['name'] in u.get('groups', [])]}
                    for g in d['user_groups']],
                'security_groups': d['security_groups'], 'catalog': [{'product': p, 'module': m, 'functions': fs} for p, m, fs in CATALOG],
                'upon_failure': list(UPON_FAILURE)}

    def save_user(self, data, previous=None):
        with self.lock:
            name = str(data.get('name', '')).strip()
            if not NAME_RE.match(name):
                raise ValueError('User name can only consist of letters, digits, spaces, ".", "_" or "-" (max 30).')
            if self.profile(data.get('profile')) is None:
                raise ValueError('Profile must be selected')
            groups = [g for g in data.get('groups', []) if any(x['name'] == g and not x.get('all_users') for x in self.doc['user_groups'])]
            u = self.user(previous) if previous else None
            if previous and u is None:
                raise ValueError(f'Unknown user: {previous}')
            if name != (previous or '') and self.user(name):
                raise ValueError('You have entered a User name that already exists.')
            if u is None:
                u = {'name': name, 'password': None, 'password_created': None, 'history': [], 'failed': [],
                     'suspended_until': None, 'grace_used': 0, 'created': _iso(_now()), 'last_login': None}
                self.doc['users'].append(u)
            if previous == 'Administrator' and name != 'Administrator':
                raise ValueError('* This is the administrative User and cannot be renamed')
            u.update({'name': name, 'full_name': str(data.get('full_name', ''))[:80], 'description': str(data.get('description', ''))[:200],
                      'profile': data['profile'], 'groups': groups, 'disabled': bool(data.get('disabled'))})
            if name == 'Administrator' and u['disabled']:
                raise ValueError('* This is the administrative User and cannot be disabled')
            if data.get('password'):
                self._set_password(u, data['password'])
            if data.get('unlock'):
                u['suspended_until'] = None
                u['failed'] = []
            self._check_admin_left()
            self._write()
            return self.public_user(u)

    def _check_admin_left(self):
        admin_fn = f'{ACC}/General/May run application'
        if not any(self.can(u, admin_fn) and not u['disabled'] for u in self.doc['users']):
            raise ValueError('At least one enabled user must keep access to the Security Manager.')

    def delete_user(self, name, current):
        with self.lock:
            if name == 'Administrator':
                raise ValueError('* This is the administrative User and cannot be deleted')
            if current and name == current['name']:
                raise ValueError('You cannot delete the user who is logged in.')
            before = len(self.doc['users'])
            self.doc['users'] = [u for u in self.doc['users'] if u['name'] != name]
            if len(self.doc['users']) == before:
                raise ValueError(f'Unknown user: {name}')
            for t, n in list(self.tickets.items()):
                if n == name:
                    del self.tickets[t]
            self._write()
            return {'ok': True}

    def save_profile(self, data, previous=None):
        with self.lock:
            name = str(data.get('name', '')).strip()
            if not name or len(name) > 60:
                raise ValueError('Invalid name, Please try again')
            rights = {}
            for k, v in (data.get('rights') or {}).items():
                if k not in FUNCTIONS and k not in MODULES and k not in PRODUCTS:
                    raise ValueError(f'Unknown access element: {k}')
                rights[k] = bool(v)
            p = self.profile(previous) if previous else None
            if name != (previous or '') and self.profile(name):
                raise ValueError('Name has to be unique.')
            if p is None:
                p = {}
                self.doc['profiles'].append(p)
            p.update({'name': name, 'description': str(data.get('description', ''))[:400],
                      'default_enable': bool(data.get('default_enable')), 'rights': rights})
            if previous and previous != name:
                for u in self.doc['users']:
                    if u['profile'] == previous:
                        u['profile'] = name
            self._check_admin_left()
            self._write()
            return p

    def delete_profile(self, name):
        with self.lock:
            if any(u['profile'] == name for u in self.doc['users']):
                raise ValueError('Cannot delete an object that is being referenced.')
            self.doc['profiles'] = [p for p in self.doc['profiles'] if p['name'] != name]
            self._write()
            return {'ok': True}

    def save_group(self, data, previous=None):
        with self.lock:
            name = str(data.get('name', '')).strip()
            if not name or len(name) > 30:
                raise ValueError('Group name must be less 30 characters, Please try again')
            g = next((g for g in self.doc['user_groups'] if g['name'] == previous), None) if previous else None
            if name != (previous or '') and any(x['name'] == name for x in self.doc['user_groups']):
                raise ValueError('Name has to be unique.')
            if g is None:
                g = {'all_users': False}
                self.doc['user_groups'].append(g)
            g.update({'name': name, 'description': str(data.get('description', ''))[:200]})
            members = set(data.get('members', []))
            for u in self.doc['users']:
                gs = [x for x in u.get('groups', []) if x not in (previous, name)]
                if u['name'] in members and not g.get('all_users'):
                    gs.append(name)
                u['groups'] = gs
            if previous and previous != name:
                for sg in self.doc['security_groups']:
                    if previous in sg['rights']:
                        sg['rights'][name] = sg['rights'].pop(previous)
            self._write()
            return g

    def delete_group(self, name):
        with self.lock:
            g = next((g for g in self.doc['user_groups'] if g['name'] == name), None)
            if g is None:
                raise ValueError(f'Unknown user group: {name}')
            if g.get('all_users'):
                raise ValueError('Cannot delete the All User UserGroup.')
            if name == 'Administrators':
                raise ValueError('Cannot delete an Access Control admin object.')
            self.doc['user_groups'].remove(g)
            for u in self.doc['users']:
                u['groups'] = [x for x in u.get('groups', []) if x != name]
            for sg in self.doc['security_groups']:
                sg['rights'].pop(name, None)
            self._write()
            return {'ok': True}

    def save_security_group(self, data, previous=None):
        with self.lock:
            name = str(data.get('name', '')).strip()
            if not name or len(name) > 60:
                raise ValueError('Invalid name, Please try again')
            names = {g['name'] for g in self.doc['user_groups']}
            rights = {}
            for ug, r in (data.get('rights') or {}).items():
                if ug not in names:
                    raise ValueError(f'Unknown user group: {ug}')
                rights[ug] = {k: bool(r.get(k)) for k in RIGHT_NAMES}
            sg = next((g for g in self.doc['security_groups'] if g['name'] == previous), None) if previous else None
            if name != (previous or '') and any(x['name'] == name for x in self.doc['security_groups']):
                raise ValueError('Name has to be unique.')
            if sg is None:
                sg = {}
                self.doc['security_groups'].append(sg)
            sg.update({'name': name, 'description': str(data.get('description', ''))[:200], 'rights': rights})
            self._write()
            return sg

    def save_policies(self, data):
        with self.lock:
            p = dict(self.doc['policies'])
            ints = {'max_failed_attempts': (1, 99), 'suspension_minutes': (1, 9999), 'lifetime_days': (1, 9999),
                    'grace_logins': (0, 99), 'passwords_kept': (0, 99), 'min_length': (1, 30), 'reuse_period_days': (0, 9999)}
            for k, (lo, hi) in ints.items():
                if k in data:
                    v = int(data[k])
                    if not lo <= v <= hi:
                        raise ValueError(f'{k} must be between {lo} and {hi}')
                    p[k] = v
            for k in ('send_log_message', 'lifetime_enabled', 'reuse_restricted'):
                if k in data:
                    p[k] = bool(data[k])
            if 'upon_failure' in data:
                if data['upon_failure'] not in UPON_FAILURE:
                    raise ValueError('Unknown failure policy')
                p['upon_failure'] = data['upon_failure']
            self.doc['policies'] = p
            self._write()
            return p

    def save_settings(self, data, current):
        with self.lock:
            s = dict(self.doc['settings'])
            if 'require_login' in data:
                want = bool(data['require_login'])
                if want and not (current and current.get('password')):
                    raise ValueError('Set a password for your own account before requiring login.')
                s['require_login'] = want
            self.doc['settings'] = s
            self._write()
            return s

    def export_security(self):
        doc = copy.deepcopy(self.doc)
        doc.pop('audit', None)
        doc.pop('esig', None)
        return json.dumps({'format': 'opentrace-security', **doc}, ensure_ascii=False, indent=1)

    def import_security(self, text, current):
        with self.lock:
            data = json.loads(text)
            if data.get('format') != 'opentrace-security' or data.get('schema') != 1:
                raise ValueError('Not an OpenTrace security export')
            for k in ('settings', 'policies', 'profiles', 'users', 'user_groups', 'security_groups'):
                if k not in data:
                    raise ValueError(f'Missing section: {k}')
            backup = copy.deepcopy(self.doc)
            for k in ('settings', 'policies', 'profiles', 'users', 'user_groups', 'security_groups'):
                self.doc[k] = data[k]
            try:
                for u in self.doc['users']:
                    if self.profile(u.get('profile')) is None:
                        raise ValueError(f"User {u.get('name')} refers to an unknown profile")
                self._check_admin_left()
            except Exception:
                self.doc = backup
                raise
            self.tickets = {t: n for t, n in self.tickets.items() if self.user(n)}
            self._write()
            return {'ok': True, 'users': len(self.doc['users'])}

    # -------------------------------------------------------- audit configuration
    def audit_state(self, map_name, attribute):
        """'on' | 'silent' | 'off' for one attribute of one map."""
        a = self.doc['audit']
        if not a['enabled']:
            return 'off'
        m = next((m for m in a['maps'] if m['name'] == map_name), None)
        if m is None or not m['enabled']:
            return 'off'
        at = next((x for x in m['attributes'] if x['name'] == attribute), None)
        if at is None or not at['auditable']:
            return 'off'
        return at['state']

    def save_audit(self, data, user):
        with self.lock:
            a = copy.deepcopy(self.doc['audit'])
            if 'enabled' in data:
                self.require(user, f'{AUDIT}/Map Configuration Tool/Enable and disable maps')
                a['enabled'] = bool(data['enabled'])
            if 'maps' in data:
                known = {m['name']: m for m in a['maps']}
                for m in data['maps']:
                    cur = known.get(m.get('name'))
                    if cur is None:
                        raise ValueError(f"Unknown audit map: {m.get('name')}")
                    if bool(m.get('enabled', cur['enabled'])) != cur['enabled']:
                        self.require(user, f'{AUDIT}/Map Configuration Tool/Enable and disable maps')
                        cur['enabled'] = bool(m['enabled'])
                    attrs = {x['name']: x for x in cur['attributes']}
                    for at in m.get('attributes', []):
                        x = attrs.get(at.get('name'))
                        if x is None:
                            raise ValueError(f"Unknown attribute {at.get('name')} of {cur['name']}")
                        st = at.get('state', x['state'])
                        if st not in AUDIT_STATES:
                            raise ValueError('Audit state must be On, Off or Silent')
                        if st != x['state'] or bool(at.get('auditable', x['auditable'])) != x['auditable']:
                            self.require(user, f'{AUDIT}/Map Configuration Tool/Change auditing state')
                        x['state'] = st
                        x['auditable'] = bool(at.get('auditable', x['auditable']))
            if 'reasons' in data:
                self.require(user, f'{AUDIT}/Map Configuration Tool/Change auditing state')
                seen = set()
                reasons = []
                for r in data['reasons']:
                    text = str(r.get('reason', '')).strip()
                    if not text:
                        raise ValueError('Values for Reason must be provided. Please try again.')
                    if text.lower() in seen:
                        raise ValueError('Audit Reason with this reason already exists, Please try again.')
                    seen.add(text.lower())
                    reasons.append({'reason': text[:500], 'description': str(r.get('description', ''))[:1000], 'active': bool(r.get('active', True))})
                a['reasons'] = reasons
            if 'free_text' in data:
                self.require(user, f'{AUDIT}/Map Configuration Tool/Change auditing state')
                a['free_text'] = bool(data['free_text'])
            old = self.doc['audit']
            self.doc['audit'] = a
            self._write()
            changes = []
            for m_old, m_new in zip(old['maps'], a['maps']):
                if m_old['enabled'] != m_new['enabled']:
                    changes.append((m_new['name'], 'enabled', m_old['enabled'], m_new['enabled']))
                for x_old, x_new in zip(m_old['attributes'], m_new['attributes']):
                    if (x_old['state'], x_old['auditable']) != (x_new['state'], x_new['auditable']):
                        changes.append((f"{m_new['name']}.{x_new['name']}", 'state',
                                        f"{x_old['state']}{'' if x_old['auditable'] else ' (not auditable)'}",
                                        f"{x_new['state']}{'' if x_new['auditable'] else ' (not auditable)'}"))
            if old['enabled'] != a['enabled']:
                changes.append(('Audit Trail', 'enabled', old['enabled'], a['enabled']))
            for inst, attr, o, n in changes:
                self._append_unlocked({'map': 'AuditMap', 'instance': inst, 'attribute': attr, 'old': str(o), 'new': str(n),
                                       'reason': str(data.get('reason', '')), 'event': 'Change Audit Setting'}, user)
            return a

    def export_maps(self):
        root = ET.Element('AuditMaps', {'enabled': str(self.doc['audit']['enabled']).lower()})
        for m in self.doc['audit']['maps']:
            me = ET.SubElement(root, 'AuditMap', {'name': m['name'], 'type': m['type'], 'enabled': str(m['enabled']).lower()})
            for a in m['attributes']:
                ET.SubElement(me, 'Attribute', {'name': a['name'], 'type': a['type'], 'state': a['state'].capitalize(),
                                                'auditable': str(a['auditable']).lower(), 'form': str(a['form'])})
        ET.indent(root)
        return '<?xml version="1.0" encoding="UTF-8"?>\n' + ET.tostring(root, encoding='unicode') + '\n'

    def parse_maps(self, xml_text):
        try:
            root = ET.fromstring(xml_text)
        except ET.ParseError as exc:
            raise ValueError(f'Not an audit map file: {exc}') from None
        if root.tag != 'AuditMaps':
            raise ValueError('Not an audit map file')
        maps = []
        for me in root.findall('AuditMap'):
            maps.append({'name': me.get('name'), 'enabled': me.get('enabled') == 'true', 'attributes': [
                {'name': a.get('name'), 'state': (a.get('state') or '').lower(), 'auditable': a.get('auditable') == 'true'}
                for a in me.findall('Attribute')]})
        return {'enabled': root.get('enabled', 'true') == 'true', 'maps': maps}

    def save_esig(self, groups, user):
        with self.lock:
            cur = {g['name']: g for g in self.doc['esig']}
            new = copy.deepcopy(self.doc['esig'])
            for g in groups:
                if g.get('name') not in cur:
                    raise ValueError(f"Unknown action group: {g.get('name')}")
                tgt = next(x for x in new if x['name'] == g['name'])
                tgt['required'] = bool(g.get('required', tgt['required']))
                ch = str(g.get('challenge', tgt['challenge']))
                if not ch.strip() or len(ch) > 1024:
                    raise ValueError('Electronic Signature Challenge Text (Max. 1024 characters)')
                tgt['challenge'] = ch
                means = {a.get('name'): a.get('meaning') for a in g.get('actions', [])}
                for a in tgt['actions']:
                    if means.get(a['name']):
                        a['meaning'] = str(means[a['name']])[:255]
            for old, g in zip(self.doc['esig'], new):
                if old['required'] != g['required']:
                    self._append_unlocked({'map': 'ESig', 'instance': g['name'], 'attribute': 'required', 'old': str(old['required']),
                                           'new': str(g['required']), 'reason': '', 'event': 'Change ESig Setting'}, user)
            self.doc['esig'] = new
            self._write()
            return new

    # -------------------------------------------------------- audit records
    def _code(self, rec):
        body = {k: rec[k] for k in sorted(rec) if k != 'hash'}
        return hmac.new(self.key, json.dumps(body, ensure_ascii=False, sort_keys=True).encode('utf-8'), hashlib.sha256).hexdigest()

    def _records(self):
        if not self.audit_path.exists():
            return []
        out = []
        with self.audit_path.open(encoding='utf-8') as fh:
            for line in fh:
                line = line.strip()
                if line:
                    out.append(json.loads(line))
        return out

    def _last(self):
        last = None
        if self.audit_path.exists():
            with self.audit_path.open('rb') as fh:
                fh.seek(0, 2)
                size = fh.tell()
                fh.seek(max(0, size - 65536))
                lines = [x for x in fh.read().splitlines() if x.strip()]
                if lines:
                    last = json.loads(lines[-1])
        return last

    def _append_unlocked(self, item, user, tz=None):
        last = self._last()
        rec = {'id': (last['id'] + 1) if last else 1, 'time': _iso(_now()), 'tz': str(tz or 'UTC')[:64],
               'user': user['name'] if user else str(item.get('instance', '')), 'full_name': (user or {}).get('full_name', ''),
               'map': str(item.get('map', ''))[:64], 'instance': str(item.get('instance', ''))[:256],
               'attribute': str(item.get('attribute', ''))[:128], 'old': str(item.get('old', ''))[:1000],
               'new': str(item.get('new', ''))[:1000], 'reason': str(item.get('reason', ''))[:1000],
               'event': str(item.get('event', ''))[:128], 'prev': last['hash'] if last else ''}
        rec['hash'] = self._code(rec)
        with self.audit_path.open('a', encoding='utf-8') as fh:
            fh.write(json.dumps(rec, ensure_ascii=False) + '\n')
        return rec

    def record(self, items, user, tz=None):
        """Append client-side audit events; each item names a map and attribute of the audit map."""
        if user is None:
            raise AccessDenied('Login required')
        if not isinstance(items, list) or len(items) > 500:
            raise ValueError('Audit records must be a list (max 500)')
        out = []
        with self.lock:
            for it in items:
                if not isinstance(it, dict):
                    raise ValueError('Audit record must be an object')
                state = self.audit_state(it.get('map'), it.get('attribute'))
                if state == 'off':
                    continue
                if state == 'on' and not str(it.get('reason', '')).strip():
                    raise ValueError('Must specify reason(s) for change')
                out.append(self._append_unlocked(it, user, tz))
        return {'recorded': len(out), 'ids': [r['id'] for r in out]}

    def history(self, q):
        recs = self._records()
        mp, inst, usr, attr = (q.get(k) or '' for k in ('map', 'instance', 'user', 'attribute'))
        text, case, whole = q.get('text') or '', bool(q.get('match_case')), bool(q.get('whole_word'))
        frm, to = q.get('from') or '', q.get('to') or ''
        if text:
            flags = 0 if case else re.I
            pat = re.compile((r'\b%s\b' if whole else '%s') % re.escape(text), flags)
        out = []
        for r in recs:
            if mp and r['map'] != mp:
                continue
            if inst and inst.lower() not in r['instance'].lower():
                continue
            if usr and r['user'] != usr:
                continue
            if attr and r['attribute'] != attr:
                continue
            day = r['time'][:10]
            if frm and day < frm or to and day > to:
                continue
            if text and not any(pat.search(str(r.get(k, ''))) for k in ('instance', 'attribute', 'old', 'new', 'reason', 'user', 'event')):
                continue
            out.append({**r, 'verified': hmac.compare_digest(self._code(r), r.get('hash', ''))})
        instances = {}
        for r in out:
            key = (r['map'], r['instance'])
            instances[key] = instances.get(key, 0) + 1
        limit = int(q.get('limit') or 5000)
        return {'records': out[-limit:], 'total': len(out),
                'instances': [{'map': m, 'instance': i, 'count': c} for (m, i), c in sorted(instances.items())],
                'users': sorted({r['user'] for r in recs}), 'maps': sorted({r['map'] for r in recs})}

    def count(self):
        return len(self._records())

    def backup(self):
        return json.dumps({'format': 'opentrace-audit', 'schema': 1, 'exported': _iso(_now()), 'records': self._records()},
                          ensure_ascii=False, indent=0)

    def restore(self, text, user):
        data = json.loads(text)
        if data.get('format') != 'opentrace-audit':
            raise ValueError('Not an OpenTrace audit backup')
        with self.lock:
            cur = self._records()
            have = {r['hash'] for r in cur}
            add = [r for r in data.get('records', []) if isinstance(r, dict) and r.get('hash') not in have]
            for r in add:
                for k in ('id', 'time', 'user', 'map', 'instance', 'attribute', 'old', 'new', 'reason', 'hash'):
                    if k not in r:
                        raise ValueError('Malformed audit record in backup')
            allr = sorted(cur + add, key=lambda r: (r['time'], r['id']))
            self._rewrite(allr)
            self._append_unlocked({'map': 'Audit', 'instance': 'Audit Records', 'attribute': 'restored', 'old': '',
                                   'new': str(len(add)), 'reason': '', 'event': 'Restore Audit Records'}, user)
            return {'restored': len(add), 'unverified': sum(not hmac.compare_digest(self._code(r), r['hash']) for r in add)}

    def _rewrite(self, records):
        tmp = self.audit_path.with_suffix('.tmp')
        with tmp.open('w', encoding='utf-8') as fh:
            for r in records:
                fh.write(json.dumps(r, ensure_ascii=False) + '\n')
        os.replace(tmp, self.audit_path)

    def delete(self, frm, to, user):
        with self.lock:
            recs = self._records()
            if frm or to:
                if not frm or not to:
                    raise ValueError('Start Date or End Date cannot be Empty!')
                if to < frm:
                    raise ValueError('End Date should be greater then Start Date!')
                keep = [r for r in recs if not frm <= r['time'][:10] <= to]
            else:
                keep = []
            self._rewrite(keep)
            n = len(recs) - len(keep)
            self._append_unlocked({'map': 'Audit', 'instance': 'Audit Records', 'attribute': 'deleted',
                                   'old': f'{frm} - {to}' if frm else 'all', 'new': str(n), 'reason': '',
                                   'event': 'Delete Audit Records'}, user)
            return {'deleted': n}

    # -------------------------------------------------------- client view
    def session_view(self, user):
        d = self.doc
        return {'require_login': d['settings']['require_login'],
                'user': self.public_user(user) if user else None,
                'groups': self.groups_of(user) if user else [],
                'rights': [k for k, v in self.rights(user).items() if v] if user else [],
                'audit': {'enabled': d['audit']['enabled'], 'maps': d['audit']['maps'], 'reasons': d['audit']['reasons'],
                          'free_text': d['audit']['free_text']},
                'esig': d['esig'], 'policies': {'min_length': d['policies']['min_length']},
                'login_users': [u['name'] for u in d['users'] if not u['disabled']],
                'security_groups': [g['name'] for g in d['security_groups']],
                'object_rights': {g['name']: sorted(self.object_rights(user, g['name'])) for g in d['security_groups']}
                if user else {}}


# ---------------------------------------------------------------- HTTP dispatch
def security_request(store: SecurityStore, ticket, request):
    if not isinstance(request, dict):
        raise ValueError('Security request must be an object')
    act = request.get('action')
    user = store.session_user(ticket)
    admin = f'{ACC}/General/May run application'
    if act == 'session':
        return store.session_view(user)
    if act == 'login':
        r = store.login(str(request.get('name', '')), str(request.get('password', '')))
        if r['ticket']:
            r.update(store.session_view(store.user(request['name'])))
        return r
    if act == 'logout':
        return store.logout(ticket)
    if act == 'change_password':
        name = request.get('name') or (user or {}).get('name')
        r = store.change_password(str(name), request.get('old'), request.get('new'))
        return r
    if act == 'verify':
        return store.verify_signature(user, request.get('name'), request.get('password'), request.get('group'), request.get('action_name'))
    if act == 'record':
        return store.record(request.get('records'), user, request.get('tz'))
    if user is None:
        raise AccessDenied('Login required')
    if act == 'admin_state':
        store.require(user, admin)
        return store.admin_state()
    if act == 'save_user':
        store.require(user, f'{ACC}/Users/Modify' if request.get('previous') else f'{ACC}/Users/Create')
        return store.save_user(request.get('user') or {}, request.get('previous'))
    if act == 'delete_user':
        store.require(user, f'{ACC}/Users/Delete')
        return store.delete_user(request.get('name'), user)
    if act == 'save_profile':
        store.require(user, f'{ACC}/Profiles/Modify' if request.get('previous') else f'{ACC}/Profiles/Create')
        return store.save_profile(request.get('profile') or {}, request.get('previous'))
    if act == 'delete_profile':
        store.require(user, f'{ACC}/Profiles/Delete')
        return store.delete_profile(request.get('name'))
    if act == 'save_group':
        store.require(user, f'{ACC}/User Groups/Modify' if request.get('previous') else f'{ACC}/User Groups/Create')
        return store.save_group(request.get('group') or {}, request.get('previous'))
    if act == 'delete_group':
        store.require(user, f'{ACC}/User Groups/Delete')
        return store.delete_group(request.get('name'))
    if act == 'save_security_group':
        store.require(user, f'{ACC}/Security Groups/Modify' if request.get('previous') else f'{ACC}/Security Groups/Create')
        return store.save_security_group(request.get('group') or {}, request.get('previous'))
    if act == 'save_policies':
        store.require(user, f'{ACC}/General/Change Password Policies')
        return store.save_policies(request.get('policies') or {})
    if act == 'save_settings':
        store.require(user, admin)
        settings = store.save_settings(request.get('settings') or {}, user)
        out = {'settings': settings}
        if settings['require_login'] and ticket not in store.tickets:
            # the administrator who turns login on stays signed in
            out['ticket'] = secrets.token_urlsafe(24)
            store.tickets[out['ticket']] = user['name']
        return out
    if act == 'export':
        store.require(user, f'{ACC}/General/Export Security Data')
        return {'text': store.export_security()}
    if act == 'import':
        store.require(user, f'{ACC}/General/Import Security Data')
        return store.import_security(request.get('text', ''), user)
    if act == 'audit_config':
        store.require(user, f'{AUDIT}/Map Configuration Tool/May run application')
        return store.doc['audit']
    if act == 'save_audit':
        store.require(user, f'{AUDIT}/Map Configuration Tool/May run application')
        return store.save_audit(request.get('audit') or {}, user)
    if act == 'export_maps':
        store.require(user, f'{AUDIT}/Map Configuration Tool/May run application')
        return {'xml': store.export_maps()}
    if act == 'parse_maps':
        store.require(user, f'{AUDIT}/Map Configuration Tool/May run application')
        return store.parse_maps(request.get('xml', ''))
    if act == 'history':
        store.require(user, f'{AUDIT}/Audit History Viewer/May run application')
        return store.history(request.get('query') or {})
    if act == 'count':
        store.require(user, f'{AUDIT}/Audit History Viewer/May run application', f'{AUDIT}/Audit Management/Backup Audit Records')
        return {'count': store.count()}
    if act == 'backup':
        store.require(user, f'{AUDIT}/Audit Management/Backup Audit Records')
        return {'text': store.backup()}
    if act == 'restore':
        store.require(user, f'{AUDIT}/Audit Management/Restore Audit Records')
        return store.restore(request.get('text', ''), user)
    if act == 'delete_records':
        store.require(user, f'{AUDIT}/Audit Management/Delete Audit Records')
        return store.delete(request.get('from') or '', request.get('to') or '', user)
    if act == 'save_esig':
        store.require(user, f'{ESIG}/ESig Administration Tool/May run application')
        return store.save_esig(request.get('groups') or [], user)
    raise ValueError(f'Unknown security action: {act}')
