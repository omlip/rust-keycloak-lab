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
    spa = load('configure_cost_monitor_spa', DIRECTORY / 'configure-cost-monitor-spa.py')


class SpaMigrationTests(unittest.TestCase):
    def test_only_login_client_changes_without_overwriting_theme_or_other_attributes(self):
        client = {'id': 'login-id', 'attributes': {'login_theme': 'keycloak.v2', 'unmanaged': 'keep'},
                  'protocolMappers': [{'name': 'existing', 'config': {}}]}
        kc = Mock()

        def respond(verb, resource, *args, **kwargs):
            if resource == 'clients':
                return [client]
            self.assertEqual(resource, 'clients/login-id')
            if verb == 'update':
                client.update(kwargs['payload'])
                client['redirectUris'] = sorted(client['redirectUris'], reverse=True)
            return client

        kc.call.side_effect = respond
        context = Mock()
        context.__enter__ = Mock(return_value=kc)
        context.__exit__ = Mock(return_value=False)
        with patch.object(spa, 'KeycloakAdmin', return_value=context):
            spa.configure()
            spa.configure()
        self.assertEqual(client['attributes']['login_theme'], 'keycloak.v2')
        self.assertEqual(client['attributes']['unmanaged'], 'keep')
        self.assertTrue(client['publicClient'])
        self.assertEqual(client['webOrigins'], ['http://localhost:8082'])
        self.assertFalse(client['directAccessGrantsEnabled'])
        self.assertFalse(client['serviceAccountsEnabled'])
        self.assertEqual(len(client['protocolMappers']), 2)
        writes = [c for c in kc.call.call_args_list if c.args[0] != 'get']
        self.assertEqual(len(writes), 2)
        for write in writes:
            self.assertNotIn('secret', write.kwargs['payload'])

    def test_import_keeps_public_login_and_confidential_offline_delegation_disjoint(self):
        realm = json.loads((helper.ROOT / 'infrastructure/keycloak/olaninc-realm.json').read_text())
        clients = {c['clientId']: c for c in realm['clients']}
        login, catalog = clients['cost-monitor-login'], clients['cost-monitor']
        self.assertTrue(login['publicClient'])
        self.assertFalse(catalog['publicClient'])
        self.assertNotIn('offline_access', login['optionalClientScopes'])
        self.assertIn('offline_access', catalog['optionalClientScopes'])
        self.assertNotIn('http://localhost:8082/oidc/login/callback', login['redirectUris'])
        self.assertEqual(login['protocolMappers'][0]['config']['included.custom.audience'], 'cost-monitor-api')


if __name__ == '__main__':
    unittest.main()
