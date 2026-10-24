"""Users, rights, password policies, audit trail and e-signatures (ID-X Admin menu)."""
import json
import tempfile
import unittest
from pathlib import Path

from genemapper_open import security
from genemapper_open.security import (SecurityStore, security_request, AccessDenied, profile_allows, DEFAULT_PROFILES,
                                      function_key)

security.ITERATIONS = 1000  # fast hashing for tests


def prof(name):
    return next(p for p in DEFAULT_PROFILES if p['name'] == name)


class DefaultModelTests(unittest.TestCase):
    def test_profiles_follow_default_gm_security(self):
        analyst, tech, sci, admin = prof('Analyst'), prof('Technician/Read Only'), prof('Scientist'), prof('Admin Profile')
        self.assertTrue(profile_allows(analyst, 'READ_ANALYSIS_METHOD'))
        self.assertTrue(profile_allows(analyst, 'UPDATE_ANALYSIS_RANGE'))
        self.assertTrue(profile_allows(analyst, 'SAVE_AS_ANALYSIS_METHOD'))
        self.assertFalse(profile_allows(analyst, 'UPDATE_ANALYSIS_METHOD'))
        self.assertTrue(profile_allows(analyst, 'UPDATE_PROJECT'))
        self.assertFalse(profile_allows(analyst, 'UPDATE_PROFILE_CONCORDANCE'))
        self.assertFalse(profile_allows(analyst, 'Remote Shutdown'))
        self.assertFalse(profile_allows(analyst, 'AdminToolAcc/General/May run application'))
        self.assertTrue(profile_allows(tech, 'READ_PROJECT'))
        self.assertFalse(profile_allows(tech, 'UPDATE_PROJECT'))
        self.assertTrue(profile_allows(sci, 'UPDATE_ANALYSIS_METHOD'))
        self.assertTrue(profile_allows(sci, 'UPDATE_PROFILE_CONCORDANCE'))
        self.assertFalse(profile_allows(sci, 'Remote Shutdown'))
        self.assertFalse(profile_allows(sci, 'AuditGUI/Audit History Viewer/May run application'))
        self.assertTrue(profile_allows(admin, 'AuditGUI/Audit Management/Delete Audit Records'))
        with self.assertRaises(ValueError):
            function_key('Create')  # ambiguous outside GeneMapper

    def test_default_store(self):
        with tempfile.TemporaryDirectory() as d:
            st = SecurityStore(d)
            names = [u['name'] for u in st.doc['users']]
            self.assertEqual(names[:2], ['Administrator', 'gmidx'])
            self.assertEqual(len(names), 10)
            self.assertEqual(st.user('Casework Tech')['profile'], 'Technician/Read Only')
            self.assertEqual(st.doc['policies']['max_failed_attempts'], 5)
            self.assertEqual(st.audit_state('Allele', 'modified'), 'on')
            self.assertEqual(st.audit_state('Sample', 'OverrideGQ'), 'silent')
            self.assertEqual(st.audit_state('Allele', 'name'), 'off')
            self.assertEqual([g['name'] for g in st.doc['esig']], ['GM_PROJECT', 'GM_CHEMISTRY_KIT', 'GM_ANALYSIS_METHOD', 'GM_SIZE_STANDARD', 'GM_MATRIX'])
            # without required login the local session acts as Administrator
            self.assertEqual(st.session_user(None)['name'], 'Administrator')
            self.assertTrue(SecurityStore(d).doc_path.exists())

    def test_session_view_lists_security_groups(self):
        # Panel Manager New Kit / Duplicate Kit offer the security groups for the kit
        with tempfile.TemporaryDirectory() as d:
            st = SecurityStore(d)
            view = st.session_view(st.session_user(None))
            self.assertIn('GeneMapper ID-X Security Group', view['security_groups'])
            self.assertEqual(len(view['security_groups']), len(st.doc['security_groups']))


class AuthenticationTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.st = SecurityStore(self.tmp.name)
        self.admin = self.st.user('Administrator')

    def tearDown(self):
        self.tmp.cleanup()

    def test_require_login_needs_own_password(self):
        with self.assertRaises(ValueError):
            self.st.save_settings({'require_login': True}, self.admin)
        self.st.save_user({'name': 'Administrator', 'full_name': 'Admin', 'profile': 'Admin Profile', 'password': 'secret1'}, 'Administrator')
        self.st.save_settings({'require_login': True}, self.admin)
        self.assertIsNone(self.st.session_user(None))
        with self.assertRaises(ValueError):
            self.st.login('Casework Analyst', 'whatever')  # no password set yet
        r = self.st.login('Administrator', 'secret1')
        self.assertEqual(self.st.session_user(r['ticket'])['name'], 'Administrator')
        self.st.logout(r['ticket'])
        self.assertIsNone(self.st.session_user(r['ticket']))

    def test_lockout_and_unlock(self):
        self.st.save_user({'name': 'Casework Analyst', 'profile': 'Analyst', 'password': 'abcdef1'}, 'Casework Analyst')
        for _ in range(5):
            with self.assertRaises(ValueError):
                self.st.login('Casework Analyst', 'wrong')
        with self.assertRaisesRegex(ValueError, 'locked out'):
            self.st.login('Casework Analyst', 'abcdef1')
        self.assertEqual(self.st.public_user(self.st.user('Casework Analyst'))['status'], 'suspended')
        self.st.save_user({'name': 'Casework Analyst', 'profile': 'Analyst', 'unlock': True}, 'Casework Analyst')
        self.assertTrue(self.st.login('Casework Analyst', 'abcdef1')['ticket'])
        self.st.save_policies({'upon_failure': 'disable', 'max_failed_attempts': 2})
        for _ in range(2):
            with self.assertRaises(ValueError):
                self.st.login('Casework Analyst', 'wrong')
        self.assertTrue(self.st.user('Casework Analyst')['disabled'])

    def test_password_policy(self):
        u = {'name': 'Practice User', 'profile': 'Scientist'}
        with self.assertRaisesRegex(ValueError, 'characters'):
            self.st.save_user({**u, 'password': 'abc'}, 'Practice User')
        with self.assertRaisesRegex(ValueError, 'spaces'):
            self.st.save_user({**u, 'password': 'abc def'}, 'Practice User')
        self.st.save_user({**u, 'password': 'first01'}, 'Practice User')
        self.st.change_password('Practice User', 'first01', 'second2')
        with self.assertRaisesRegex(ValueError, 'used earlier'):
            self.st.change_password('Practice User', 'second2', 'first01')
        with self.assertRaisesRegex(ValueError, 'Old Password'):
            self.st.change_password('Practice User', 'nope', 'third03')
        self.st.save_policies({'lifetime_enabled': True, 'lifetime_days': 1, 'grace_logins': 1})
        self.st.user('Practice User')['password_created'] = '2020-01-01T00:00:00Z'
        self.assertTrue(self.st.login('Practice User', 'second2')['ticket'])  # one grace login
        r = self.st.login('Practice User', 'second2')
        self.assertTrue(r['must_change_password'])
        self.assertIsNone(r['ticket'])

    def test_admin_objects_protected(self):
        with self.assertRaises(ValueError):
            self.st.delete_user('Administrator', None)
        with self.assertRaises(ValueError):
            self.st.delete_group('All Users')
        with self.assertRaises(ValueError):
            self.st.delete_profile('Analyst')  # referenced
        self.st.save_profile({'name': 'Reviewer', 'description': 'x', 'rights': {'GeneMapper ID-X/Project/READ_PROJECT': True}})
        self.assertTrue(profile_allows(self.st.profile('Reviewer'), 'READ_PROJECT'))
        with self.assertRaises(ValueError):
            self.st.save_profile({'name': 'Bad', 'rights': {'Nope': True}})
        self.st.save_group({'name': 'QA', 'members': ['Casework Tech']})
        self.assertIn('QA', self.st.groups_of(self.st.user('Casework Tech')))
        self.assertIn('All Users', self.st.groups_of(self.st.user('Casework Tech')))
        with self.assertRaisesRegex(ValueError, 'Security Manager'):
            for u in list(self.st.doc['users']):
                if u['profile'] == 'Admin Profile':
                    self.st.save_user({'name': u['name'], 'profile': 'Analyst'}, u['name'])

    def test_export_import(self):
        text = self.st.export_security()
        self.assertNotIn('audit', json.loads(text))
        self.st.save_user({'name': 'Temp', 'profile': 'Analyst'})
        self.st.import_security(text, self.admin)
        self.assertIsNone(self.st.user('Temp'))
        with self.assertRaises(ValueError):
            self.st.import_security('{"format":"x"}', self.admin)


class AuditTrailTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.st = SecurityStore(self.tmp.name)
        self.admin = self.st.user('Administrator')

    def tearDown(self):
        self.tmp.cleanup()

    def test_states_reasons_and_verification(self):
        with self.assertRaisesRegex(ValueError, 'reason'):
            self.st.record([{'map': 'Allele', 'attribute': 'modified', 'instance': 'S1', 'old': '12', 'new': '13'}], self.admin)
        r = self.st.record([{'map': 'Allele', 'attribute': 'modified', 'instance': 'Run/S1.fsa · D3', 'old': '12', 'new': '13', 'reason': 'Discrepancy'},
                            {'map': 'Sample', 'attribute': 'OverrideGQ', 'instance': 'Run/S1.fsa · D3', 'old': '', 'new': '1.0'},
                            {'map': 'Allele', 'attribute': 'name', 'instance': 'x'}], self.admin, 'Europe/Moscow')
        self.assertEqual(r['recorded'], 2)
        h = self.st.history({})
        self.assertEqual([x['attribute'] for x in h['records']], ['modified', 'OverrideGQ'])
        self.assertTrue(all(x['verified'] for x in h['records']))
        self.assertEqual(h['records'][0]['tz'], 'Europe/Moscow')
        self.assertEqual(h['records'][1]['prev'], h['records'][0]['hash'])
        self.assertEqual(self.st.history({'text': 'discrepancy'})['total'], 1)
        self.assertEqual(self.st.history({'text': 'discrepancy', 'match_case': True})['total'], 0)
        self.assertEqual(self.st.history({'map': 'Sample'})['total'], 1)
        # tampering breaks the HMAC
        lines = self.st.audit_path.read_text(encoding='utf-8').splitlines()
        rec = json.loads(lines[0])
        rec['new'] = '14'
        lines[0] = json.dumps(rec)
        self.st.audit_path.write_text('\n'.join(lines) + '\n', encoding='utf-8')
        self.assertEqual([x['verified'] for x in self.st.history({})['records']], [False, True])

    def test_map_configuration_rights_and_xml(self):
        analyst = self.st.user('Casework Analyst')
        with self.assertRaises(AccessDenied):
            self.st.save_audit({'maps': [{'name': 'Allele', 'attributes': [{'name': 'modified', 'state': 'off'}]}]}, analyst)
        self.st.save_audit({'maps': [{'name': 'Allele', 'attributes': [{'name': 'modified', 'state': 'silent'}]}],
                            'reasons': [{'reason': 'Discrepancy'}, {'reason': 'Error', 'active': False}], 'free_text': False}, self.admin)
        self.assertEqual(self.st.audit_state('Allele', 'modified'), 'silent')
        self.assertEqual(self.st.history({'map': 'AuditMap'})['records'][0]['new'], 'silent')
        with self.assertRaisesRegex(ValueError, 'already exists'):
            self.st.save_audit({'reasons': [{'reason': 'A'}, {'reason': 'a'}]}, self.admin)
        xml = self.st.export_maps()
        parsed = self.st.parse_maps(xml)
        allele = next(m for m in parsed['maps'] if m['name'] == 'Allele')
        self.assertEqual(next(a for a in allele['attributes'] if a['name'] == 'modified')['state'], 'silent')
        self.st.save_audit({'enabled': False}, self.admin)
        self.assertEqual(self.st.audit_state('Allele', 'created'), 'off')
        with self.assertRaises(ValueError):
            self.st.parse_maps('<x/>')

    def test_backup_delete_restore(self):
        self.st.record([{'map': 'Project', 'attribute': 'modified', 'instance': 'P1'}], self.admin)
        self.st.record([{'map': 'Project', 'attribute': 'analyzed', 'instance': 'P1'}], self.admin)
        backup = self.st.backup()
        self.assertEqual(self.st.count(), 2)
        today = self.st.history({})['records'][0]['time'][:10]
        with self.assertRaises(ValueError):
            self.st.delete(today, '', self.admin)
        self.assertEqual(self.st.delete(today, today, self.admin)['deleted'], 2)
        self.assertEqual(self.st.count(), 1)  # the deletion itself is recorded
        r = self.st.restore(backup, self.admin)
        self.assertEqual(r, {'restored': 2, 'unverified': 0})
        self.assertEqual(self.st.count(), 4)
        self.assertEqual(self.st.restore(backup, self.admin)['restored'], 0)

    def test_esignature(self):
        with self.assertRaisesRegex(ValueError, 'logged-in'):
            self.st.verify_signature(self.admin, 'Lab Director', 'x', 'GM_PROJECT', 'MODIFY_PROJECT')
        self.st.save_user({'name': 'Administrator', 'profile': 'Admin Profile', 'password': 'signit1'}, 'Administrator')
        with self.assertRaises(ValueError):
            self.st.verify_signature(self.admin, 'Administrator', 'bad', 'GM_PROJECT', 'MODIFY_PROJECT')
        r = self.st.verify_signature(self.admin, 'Administrator', 'signit1', 'GM_PROJECT', 'MODIFY_PROJECT')
        self.assertEqual(r['meaning'], 'Modify Project')
        self.assertEqual(self.st.history({'map': 'ESig'})['records'][0]['attribute'], 'MODIFY_PROJECT')
        self.st.save_esig([{'name': 'GM_ANALYSIS_METHOD', 'required': True}], self.admin)
        self.assertTrue(next(g for g in self.st.doc['esig'] if g['name'] == 'GM_ANALYSIS_METHOD')['required'])
        with self.assertRaises(ValueError):
            self.st.save_esig([{'name': 'GM_X'}], self.admin)


class DispatchTests(unittest.TestCase):
    def test_request_rights(self):
        with tempfile.TemporaryDirectory() as d:
            st = SecurityStore(Path(d) / 'sec')
            view = security_request(st, '', {'action': 'session'})
            self.assertFalse(view['require_login'])
            self.assertIn('GeneMapper ID-X/Project/UPDATE_PROJECT', view['rights'])
            self.assertEqual(security_request(st, '', {'action': 'admin_state'})['users'][0]['name'], 'Administrator')
            security_request(st, '', {'action': 'save_user', 'previous': 'Casework Tech',
                                      'user': {'name': 'Casework Tech', 'profile': 'Technician/Read Only', 'password': 'tech123'}})
            security_request(st, '', {'action': 'save_user', 'previous': 'Administrator',
                                      'user': {'name': 'Administrator', 'profile': 'Admin Profile', 'password': 'admin12'}})
            security_request(st, '', {'action': 'save_settings', 'settings': {'require_login': True}})
            with self.assertRaises(AccessDenied):
                security_request(st, '', {'action': 'admin_state'})
            t = security_request(st, '', {'action': 'login', 'name': 'Casework Tech', 'password': 'tech123'})
            self.assertNotIn('GeneMapper ID-X/Project/UPDATE_PROJECT', t['rights'])
            with self.assertRaises(AccessDenied):
                security_request(st, t['ticket'], {'action': 'history'})
            with self.assertRaises(AccessDenied):
                st.require(st.session_user(t['ticket']), 'UPDATE_PROJECT')
            with self.assertRaises(ValueError):
                security_request(st, t['ticket'], {'action': 'nope'})


if __name__ == '__main__':
    unittest.main()
