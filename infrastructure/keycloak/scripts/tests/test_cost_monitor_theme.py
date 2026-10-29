import importlib.util
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import Mock, call, patch
from xml.etree import ElementTree


DIRECTORY = Path(__file__).resolve().parents[1]
THEME = 'cost-moinitor-login-theme'


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


helper = load('keycloak_admin', DIRECTORY / 'keycloak_admin.py')
with patch.dict(sys.modules, {'keycloak_admin': helper}):
    theme = load('configure_cost_monitor_theme', DIRECTORY / 'configure-cost-monitor-theme.py')


class ClientThemeTests(unittest.TestCase):
    def setUp(self):
        self.clients = {
            name: {'id': name + '-id', 'attributes': {'pkce.code.challenge.method': 'S256',
                    'unmanaged-setting': name, 'post.logout.redirect.uris': 'http://localhost:8082/'}}
            for name in ('cost-monitor-login', 'cost-monitor')
        }
        self.kc = Mock()
        self.kc.call.side_effect = self.respond
        context = Mock()
        context.__enter__ = Mock(return_value=self.kc)
        context.__exit__ = Mock(return_value=False)
        patcher = patch.object(theme, 'KeycloakAdmin', return_value=context)
        patcher.start()
        self.addCleanup(patcher.stop)

    def respond(self, verb, resource, *args, **kwargs):
        if resource == 'serverinfo':
            return {'themes': {'login': [{'name': THEME}]}}
        if resource == 'clients':
            return [self.clients[args[1].split('=', 1)[1]]]
        client = next(c for c in self.clients.values() if resource == 'clients/' + c['id'])
        if verb == 'update':
            client.update(kwargs['payload'])
        return client

    def test_only_two_client_theme_attributes_change_and_replay_is_safe(self):
        before = json.loads(json.dumps(self.clients))
        for _ in range(2):
            theme.configure()
        for name, client in self.clients.items():
            expected = {**before[name]['attributes'], 'login_theme': THEME}
            self.assertEqual(client['attributes'], expected)
        writes = [c for c in self.kc.call.call_args_list if c.args[0] != 'get']
        self.assertEqual(len(writes), 4)
        for c in writes:
            self.assertEqual(c.args[0], 'update')
            self.assertEqual(set(c.kwargs['payload']), {'attributes'})
            self.assertIn(c.args[1], ['clients/' + c['id'] for c in self.clients.values()])

    def test_missing_theme_stops_before_any_write(self):
        self.kc.call.side_effect = [{'themes': {'login': []}}]
        with self.assertRaisesRegex(RuntimeError, 'not available'):
            theme.configure()
        self.assertEqual(self.kc.call.call_args_list, [call('get', 'serverinfo')])

    def test_missing_second_client_stops_before_any_write(self):
        respond = self.respond
        self.kc.call.side_effect = lambda *a, **kw: [] if a[-1] == 'clientId=cost-monitor' else respond(*a, **kw)
        with self.assertRaisesRegex(RuntimeError, 'exactly one client'):
            theme.configure()
        self.assertTrue(all(c.args[0] == 'get' for c in self.kc.call.call_args_list))

    def test_failed_readback_is_reported(self):
        respond = self.respond
        self.kc.call.side_effect = lambda *a, **kw: {'attributes': {}} if a[:2] == ('get', 'clients/cost-monitor-login-id') else respond(*a, **kw)
        with self.assertRaisesRegex(RuntimeError, 'verification failed'):
            theme.configure()

    def test_login_only_resources_and_realm_default_remain_distinct(self):
        root = helper.ROOT / 'infrastructure/keycloak/themes' / THEME
        self.assertEqual({p.name for p in root.iterdir()}, {'login'})
        directory = root / 'login'
        props = dict(line.split('=', 1) for line in (directory / 'theme.properties').read_text().splitlines())
        self.assertEqual(props['parent'], 'keycloak.v2')
        self.assertEqual(props['darkMode'], 'false')
        self.assertTrue((directory / 'resources/css/cost-monitor.css').is_file())
        self.assertEqual(ElementTree.parse(directory / 'resources' / props['favicons.logo']).getroot().tag,
                         '{http://www.w3.org/2000/svg}svg')
        self.assertFalse(list(root.rglob('*.ftl')))
        self.assertFalse(list(root.rglob('*.js')))
        for locale in ('fr', 'en'):
            self.assertIn('loginTitleHtml=Cost Monitor', (directory / 'messages' / ('messages_' + locale + '.properties')).read_text())
        realm = json.loads((helper.ROOT / 'infrastructure/keycloak/olaninc-realm.json').read_text())
        for kind in ('login', 'account', 'admin'):
            self.assertEqual(realm[kind + 'Theme'], 'ni2')
        overrides = {c['clientId']: c.get('attributes', {}).get('login_theme') for c in realm['clients']}
        self.assertEqual({name: value for name, value in overrides.items() if value},
                         {'cost-monitor-login': THEME, 'cost-monitor': THEME})


if __name__ == '__main__':
    unittest.main()
