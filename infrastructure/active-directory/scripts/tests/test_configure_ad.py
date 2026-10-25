import importlib.util
import json
from pathlib import Path
import runpy
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch


SPEC = importlib.util.spec_from_file_location('configure_ad', Path(__file__).resolve().parents[1] / 'configure-ad.py')
ad = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ad)


class CredentialsTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='olaninc-ad-')
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / '.secrets/ad-credentials.json'
        self.patch_path = patch.object(ad, 'SECRET_FILE', self.path)
        self.patch_path.start()
        self.addCleanup(self.patch_path.stop)

    def test_generates_distinct_protected_passwords_for_exact_accounts(self):
        with patch.object(ad, 'run', return_value=''):
            values = ad.credentials()
        self.assertEqual(set(values), set(ad.ACCOUNTS))
        self.assertEqual(len(set(values.values())), len(values))
        self.assertEqual(self.path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(self.path.parent.stat().st_mode & 0o777, 0o700)
        self.assertTrue(all(len(value) >= 16 for value in values.values()))

    def test_replay_preserves_file_and_passwords_without_docker_lookup(self):
        with patch.object(ad, 'run', return_value=''):
            expected = ad.credentials()
        before = self.path.read_bytes()
        with patch.object(ad, 'run') as run:
            self.assertEqual(ad.credentials(), expected)
            run.assert_not_called()
        self.assertEqual(self.path.read_bytes(), before)

    def test_missing_secrets_with_persisted_domain_never_regenerates(self):
        with patch.object(ad, 'run', return_value='olaninc-iam-demo_samba-data\n'):
            with self.assertRaisesRegex(RuntimeError, 'Restore'):
                ad.credentials()
        self.assertFalse(self.path.exists())

    def test_rejects_symlink_without_overwriting_target(self):
        self.path.parent.mkdir()
        target = Path(self.directory.name) / 'target'
        target.write_text('do not overwrite')
        self.path.symlink_to(target)
        with self.assertRaisesRegex(RuntimeError, 'symlink'):
            ad.credentials()
        self.assertEqual(target.read_text(), 'do not overwrite')

    def test_rejects_incomplete_credentials_without_replacing_file(self):
        self.path.parent.mkdir()
        self.path.write_text(json.dumps({'user1': 'not-enough-accounts'}))
        before = self.path.read_bytes()
        with self.assertRaisesRegex(RuntimeError, 'Invalid'):
            ad.credentials()
        self.assertEqual(self.path.read_bytes(), before)


class FederationTests(unittest.TestCase):
    def test_local_username_collision_stops_before_any_mutation(self):
        kc = Mock()
        kc.call.side_effect = [
            {'id': 'realm-id'}, [], [{'id': 'local-user', 'username': 'user1'}],
        ]
        with self.assertRaisesRegex(RuntimeError, 'collision'):
            ad.configure_federation(kc, {})
        self.assertTrue(all(call.args[0] == 'get' for call in kc.call.call_args_list))

    def test_config_encodes_native_keycloak_multivalue_properties(self):
        self.assertEqual(ad.config({'editMode': 'READ_ONLY', 'searchScope': '2'}),
                         {'editMode': ['READ_ONLY'], 'searchScope': ['2']})


class ProjectLayoutTests(unittest.TestCase):
    def test_ad_scripts_resolve_the_repository_not_the_component_directory(self):
        root = ad.ROOT
        self.assertTrue((root / 'compose.yaml').is_file())
        self.assertTrue((root / 'catalog-services/backend/Cargo.toml').is_file())
        self.assertEqual(ad.SECRET_FILE, root / '.secrets/ad-credentials.json')
        self.assertEqual(ad.CA_FILE, root / 'infrastructure/keycloak/truststores/ad-ca.crt')
        verify = runpy.run_path(str(Path(__file__).resolve().parents[1] / 'verify-ad.py'))
        self.assertEqual(verify['ROOT'], root)

    def test_keycloak_helpers_resolve_the_same_root_and_keep_local_imports_working(self):
        directory = ad.ROOT / 'infrastructure/keycloak/scripts'
        spec = importlib.util.spec_from_file_location('keycloak_admin', directory / 'keycloak_admin.py')
        helper = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(helper)
        self.assertEqual(helper.ROOT, ad.ROOT)
        self.assertIn(str(ad.ROOT / 'compose.yaml'), helper.COMPOSE)
        loader = runpy.run_path(str(directory / 'configure-catalog-loader.py'))
        self.assertEqual(loader['ROOT'], ad.ROOT)
        self.assertEqual(loader['SECRET_FILE'], ad.ROOT / '.secrets/catalog-loader.secret')
        with patch.dict(sys.modules, {'keycloak_admin': helper}):
            monitor = runpy.run_path(str(directory / 'configure-cost-monitor.py'))
        self.assertEqual(monitor['ROOT'], ad.ROOT)


if __name__ == '__main__':
    unittest.main()
