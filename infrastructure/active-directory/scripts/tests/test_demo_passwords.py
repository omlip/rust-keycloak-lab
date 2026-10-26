import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location('set_demo_passwords', Path(__file__).resolve().parents[1] / 'set-demo-passwords.py')
demo = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(demo)


class DemoPasswordsTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='demo-passwords-')
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / 'credentials.json'
        self.values = {name: 'Initial-secret-' + name + '-2026!' for name in demo.ACCOUNTS}
        self.path.write_text(json.dumps(self.values))
        self.patcher = patch.object(demo, 'SECRET_FILE', self.path)
        self.patcher.start()
        self.addCleanup(self.patcher.stop)

    def test_updates_only_human_passwords_preserving_inode_technical_secrets_and_hashes(self):
        original = self.path.read_bytes()
        inode = self.path.stat().st_ino
        with patch.object(demo, 'update_ad') as update:
            demo.configure()
        values = json.loads(self.path.read_text())
        for name in demo.USERS:
            self.assertEqual(values[name], name + '-12345!')
        for name in ('Administrator', 'svc-keycloak'):
            self.assertEqual(values[name], self.values[name])
        update.assert_called_once_with({name: name + '-12345!' for name in demo.USERS},
                                       hashlib.sha256(original).hexdigest(),
                                       hashlib.sha256(self.path.read_bytes()).hexdigest())
        self.assertEqual(self.path.stat().st_ino, inode)
        self.assertEqual(self.path.stat().st_mode & 0o777, 0o600)

    def test_failed_ad_update_leaves_credentials_file_unchanged(self):
        original = self.path.read_bytes()
        with patch.object(demo, 'update_ad', side_effect=RuntimeError('AD unavailable')):
            with self.assertRaises(RuntimeError):
                demo.configure()
        self.assertEqual(self.path.read_bytes(), original)

    def test_replay_verifies_marker_without_resetting_passwords(self):
        self.values.update({name: name + '-12345!' for name in demo.USERS})
        self.path.write_text(json.dumps(self.values))
        digest = hashlib.sha256(self.path.read_bytes()).hexdigest()
        with patch.object(demo, 'update_ad') as update:
            demo.configure()
        update.assert_called_once_with({}, digest, digest)

    def test_remote_script_is_valid_python(self):
        compile(demo.REMOTE, '<ad-password-maintenance>', 'exec')

    def test_short_technical_secret_is_rejected_before_ad_changes(self):
        self.values['svc-keycloak'] = 'short-12345!'
        self.path.write_text(json.dumps(self.values))
        original = self.path.read_bytes()
        with patch.object(demo, 'update_ad') as update:
            with self.assertRaisesRegex(RuntimeError, 'Invalid'):
                demo.configure()
        update.assert_not_called()
        self.assertEqual(self.path.read_bytes(), original)


if __name__ == '__main__':
    unittest.main()
