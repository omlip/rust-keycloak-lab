import importlib.util
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import Mock, patch
from xml.etree import ElementTree


DIRECTORY = Path(__file__).resolve().parents[1]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


helper = load('keycloak_admin', DIRECTORY / 'keycloak_admin.py')
with patch.dict(sys.modules, {'keycloak_admin': helper}):
    theme = load('configure_theme', DIRECTORY / 'configure-theme.py')


class ThemeConfigurationTests(unittest.TestCase):
    def setUp(self):
        self.settings = {'loginTheme': 'ni2', 'accountTheme': 'ni2', 'adminTheme': 'ni2'}
        self.kc = Mock()
        self.kc.call.side_effect = [
            {'themes': {kind: [{'name': 'ni2'}] for kind in ('login', 'account', 'admin')}},
            None, self.settings,
        ]
        context = Mock()
        context.__enter__ = Mock(return_value=self.kc)
        context.__exit__ = Mock(return_value=False)
        patcher = patch.object(theme, 'KeycloakAdmin', return_value=context)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_only_three_theme_fields_of_olaninc_are_updated(self):
        theme.configure()
        writes = [call for call in self.kc.call.call_args_list if call.args[0] != 'get']
        self.assertEqual(len(writes), 1)
        self.kc.call.assert_any_call('update', 'realms/olaninc', payload=self.settings)

    def test_missing_theme_stops_before_any_realm_mutation(self):
        self.kc.call.side_effect = [{'themes': {kind: [] for kind in ('login', 'account', 'admin')}}]
        with self.assertRaisesRegex(RuntimeError, 'not available'):
            theme.configure()
        self.assertTrue(all(call.args[0] == 'get' for call in self.kc.call.call_args_list))

    def test_readback_mismatch_is_reported(self):
        self.kc.call.side_effect = [
            {'themes': {kind: [{'name': 'ni2'}] for kind in ('login', 'account', 'admin')}},
            None, {'loginTheme': 'ni2', 'accountTheme': 'keycloak.v3', 'adminTheme': 'ni2'},
        ]
        with self.assertRaisesRegex(RuntimeError, 'verification failed'):
            theme.configure()


class ThemeResourcesTests(unittest.TestCase):
    def test_console_themes_inherit_native_apps_and_reference_existing_resources(self):
        root = helper.ROOT / 'infrastructure/keycloak/themes/ni2'
        for kind, parent in [('account', 'keycloak.v3'), ('admin', 'keycloak.v2')]:
            directory = root / kind
            props = dict(line.split('=', 1) for line in (directory / 'theme.properties').read_text().splitlines()
                         if line and not line.startswith('#'))
            self.assertEqual(props['parent'], parent)
            self.assertEqual(props['darkMode'], 'false')
            for resource in (props['styles'], props['logo'], props['favicons.logo']):
                self.assertTrue((directory / 'resources' / resource).is_file())
            self.assertFalse(list(directory.rglob('*.ftl')))
            self.assertFalse(list(directory.rglob('*.js')))
            for image in directory.rglob('*.svg'):
                self.assertEqual(ElementTree.parse(image).getroot().tag, '{http://www.w3.org/2000/svg}svg')

    def test_import_selects_the_three_theme_types_without_changing_email_theme(self):
        realm = json.loads((helper.ROOT / 'infrastructure/keycloak/olaninc-realm.json').read_text())
        for kind in ('login', 'account', 'admin'):
            self.assertEqual(realm[kind + 'Theme'], 'ni2')
        self.assertNotIn('emailTheme', realm)


if __name__ == '__main__':
    unittest.main()
