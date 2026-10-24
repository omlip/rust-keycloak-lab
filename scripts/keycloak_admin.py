"""Small maintenance-only wrapper around Keycloak's official CLI.

Never print stdout or request bodies: they may contain credentials.
"""
import json
import subprocess
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
COMPOSE = ['docker', 'compose', '-f', str(ROOT / 'compose.yaml')]


class KeycloakAdmin:
    def __init__(self):
        self.config = '/tmp/cost-monitor-admin-' + uuid.uuid4().hex + '.json'

    def __enter__(self):
        result = subprocess.run(COMPOSE + ['exec', '-T', '-e', 'KEYCLOAK_ADMIN_USER', '-e', 'KEYCLOAK_ADMIN_PASSWORD',
            'keycloak', 'sh', '-c',
            'umask 077; /opt/keycloak/bin/kcadm.sh config credentials --config "$1" '
            '--server http://localhost:8080 --realm master '
            '--user "${KEYCLOAK_ADMIN_USER:-$KC_BOOTSTRAP_ADMIN_USERNAME}" '
            '--password "${KEYCLOAK_ADMIN_PASSWORD:-$KC_BOOTSTRAP_ADMIN_PASSWORD}"',
            'sh', self.config], cwd=ROOT, capture_output=True)
        if result.returncode:
            self.__exit__(None, None, None)
            raise RuntimeError('Keycloak admin login failed; provide valid administration credentials.')
        return self

    def call(self, action, path, *args, payload=None):
        command = COMPOSE + ['exec', '-T', 'keycloak', '/opt/keycloak/bin/kcadm.sh',
                             action, path, *args, '--config', self.config, '-r', 'olaninc']
        if payload is not None:
            command += ['-f', '-']
        result = subprocess.run(command, cwd=ROOT, text=True, capture_output=True,
                                input=None if payload is None else json.dumps(payload))
        if result.returncode:
            raise RuntimeError(f'Keycloak operation failed: {action} {path}')
        return json.loads(result.stdout) if result.stdout.strip() else None

    def __exit__(self, *_):
        subprocess.run(COMPOSE + ['exec', '-T', 'keycloak', 'rm', '-f', self.config], cwd=ROOT, capture_output=True)
