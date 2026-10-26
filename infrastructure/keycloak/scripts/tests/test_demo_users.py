import importlib.util
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import Mock, patch


DIRECTORY = Path(__file__).resolve().parents[1]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


helper = load('keycloak_admin', DIRECTORY / 'keycloak_admin.py')
with patch.dict(sys.modules, {'keycloak_admin': helper}):
    demo = load('configure_demo_users', DIRECTORY / 'configure-demo-users.py')


class DemoUsersTests(unittest.TestCase):
    def setUp(self):
        self.users = {name: {'id': name, 'username': name,
                            'requiredActions': ['UPDATE_PASSWORD', 'CONFIGURE_TOTP']}
                      for name in demo.USERNAMES}
        self.kc = Mock()
        self.kc.call.side_effect = self.call
        context = Mock()
        context.__enter__ = Mock(return_value=self.kc)
        context.__exit__ = Mock(return_value=False)
        self.patcher = patch.object(demo, 'KeycloakAdmin', return_value=context)
        self.patcher.start()
        self.addCleanup(self.patcher.stop)

    def call(self, action, path, *args, payload=None):
        if action == 'get' and path == 'users':
            name = next(arg.split('=', 1)[1] for arg in args if arg.startswith('username='))
            return [self.users[name]] if name in self.users else []
        if action == 'create' and path == 'users':
            self.users[payload['username']] = dict(payload, id=payload['username'])
        if action == 'get' and path == 'clients':
            return [{'id': 'realm-management'}]
        if action == 'get' and ('/roles/' in path or path.startswith('roles/')):
            return {'id': path, 'name': path.rsplit('/', 1)[1]}

    def test_replay_never_resets_existing_passwords_or_credentials(self):
        demo.configure()
        self.assertFalse(any('/reset-password' in call.args[1] for call in self.kc.call.call_args_list))
        self.assertFalse(any(call.args[0] == 'delete' for call in self.kc.call.call_args_list))

    def test_explicit_reset_keeps_otp_required_action_and_uses_public_demo_password(self):
        demo.configure(reset_passwords=True)
        for name in demo.USERNAMES:
            self.kc.call.assert_any_call('update', 'users/' + name + '/reset-password',
                                        payload={'type': 'password', 'value': name + '-12345!', 'temporary': False})
            self.kc.call.assert_any_call('update', 'users/' + name,
                                        payload={'requiredActions': ['CONFIGURE_TOTP']})
        self.assertFalse(any(call.args[0] == 'delete' for call in self.kc.call.call_args_list))

    def test_new_personas_are_created_without_administration_roles(self):
        del self.users['demo-security']
        del self.users['demo-bruteforce']
        demo.configure()
        for name in ('demo-security', 'demo-bruteforce'):
            self.assertFalse(self.users[name]['credentials'][0]['temporary'])
            self.assertFalse(any('users/' + name + '/role-mappings/clients/' in call.args[1]
                                 for call in self.kc.call.call_args_list))

    def test_federated_collision_stops_before_any_mutation(self):
        self.users['demo-security']['federationLink'] = 'ldap-provider'
        with self.assertRaisesRegex(RuntimeError, 'local demo account'):
            demo.configure(reset_passwords=True)
        self.assertTrue(all(call.args[0] == 'get' for call in self.kc.call.call_args_list))

    def test_import_passwords_match_documented_convention(self):
        realm = json.loads((helper.ROOT / 'infrastructure/keycloak/olaninc-realm.json').read_text())
        for name in demo.USERNAMES:
            user = next(user for user in realm['users'] if user['username'] == name)
            self.assertEqual(user['credentials'][0]['value'], name + '-12345!')
            self.assertGreaterEqual(len(user['credentials'][0]['value']), 12)
            self.assertEqual(user['requiredActions'], [])


if __name__ == '__main__':
    unittest.main()
