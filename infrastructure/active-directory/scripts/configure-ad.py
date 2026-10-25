#!/usr/bin/env python3
"""Provision the optional Samba demo and configure native Keycloak federation.

Run from any directory. Secrets stay in .secrets/ and are never printed.
Existing accounts, realm settings and AD passwords are not reset on replay.
"""

import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import uuid


ROOT = Path(__file__).resolve().parents[3]
COMPOSE = ['docker', 'compose', '-f', str(ROOT / 'compose.yaml')]
SECRET_FILE = ROOT / '.secrets/ad-credentials.json'
CA_FILE = ROOT / 'infrastructure/keycloak/truststores/ad-ca.crt'
USERS = ['user1', 'user2', 'user3', 'user4', 'admin1']
ACCOUNTS = ['Administrator', 'svc-keycloak', *USERS]
BASE_DN = 'DC=ad,DC=olaninc,DC=test'
PROVIDER_NAME = 'olaninc-samba-ad'


def run(args, *, data=None):
    result = subprocess.run(args, cwd=ROOT, input=data, text=True, capture_output=True)
    if result.returncode:
        # Command bodies and stdout may contain secrets. Do not print them.
        raise RuntimeError('Command failed: ' + ' '.join(args[:3]))
    return result.stdout


def credentials():
    if SECRET_FILE.parent.is_symlink() or SECRET_FILE.is_symlink():
        raise RuntimeError('The secrets directory/file must not be a symlink.')
    SECRET_FILE.parent.mkdir(mode=0o700, exist_ok=True)
    SECRET_FILE.parent.chmod(0o700)
    if not SECRET_FILE.exists():
        # Never invent replacement passwords when a persisted domain exists.
        volumes = run(['docker', 'volume', 'ls',
                       '--filter', 'label=com.docker.compose.project=olaninc-iam-demo',
                       '--filter', 'label=com.docker.compose.volume=samba-data',
                       '--format', '{{.Name}}']).splitlines()
        if any(name.endswith('_samba-data') for name in volumes):
            raise RuntimeError('Samba volume exists but credentials are missing. '
                               'Restore .secrets/ad-credentials.json; no passwords were reset.')
        values = {name: 'Demo-' + secrets.token_urlsafe(20) + '-Aa9!' for name in ACCOUNTS}
        fd = os.open(SECRET_FILE, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, 'w') as stream:
            json.dump(values, stream, indent=2)
            stream.write('\n')
    SECRET_FILE.chmod(0o600)
    values = json.loads(SECRET_FILE.read_text())
    if set(values) != set(ACCOUNTS) or any(not isinstance(v, str) or len(v) < 16 for v in values.values()):
        raise RuntimeError('Invalid AD credentials file; refusing to replace it.')
    return values


class Keycloak:
    def __init__(self):
        self.config = '/tmp/olaninc-ad-' + uuid.uuid4().hex + '.json'

    def login(self):
        run(COMPOSE + ['exec', '-T', '-e', 'KEYCLOAK_ADMIN_USER', '-e', 'KEYCLOAK_ADMIN_PASSWORD',
            'keycloak', 'sh', '-c',
            'umask 077; /opt/keycloak/bin/kcadm.sh config credentials --config "$1" '
            '--server http://localhost:8080 --realm master '
            '--user "${KEYCLOAK_ADMIN_USER:-$KC_BOOTSTRAP_ADMIN_USERNAME}" '
            '--password "${KEYCLOAK_ADMIN_PASSWORD:-$KC_BOOTSTRAP_ADMIN_PASSWORD}"',
            'sh', self.config])

    def call(self, action, path, *args, payload=None):
        cmd = COMPOSE + ['exec', '-T', 'keycloak', '/opt/keycloak/bin/kcadm.sh',
                        action, path, *args, '--config', self.config, '-r', 'olaninc']
        if payload is not None:
            cmd += ['-f', '-']
        text = run(cmd, data=None if payload is None else json.dumps(payload))
        return json.loads(text) if text.strip() else None

    def close(self):
        run(COMPOSE + ['exec', '-T', 'keycloak', 'rm', '-f', self.config])


def config(values):
    return {key: [str(value)] for key, value in values.items()}


def configure_federation(kc, values):
    realm_id = kc.call('get', 'realms/olaninc')['id']
    providers = kc.call('get', 'components', '-q', 'parent=' + realm_id,
                        '-q', 'type=org.keycloak.storage.UserStorageProvider')
    matching = [p for p in providers if p['name'] == PROVIDER_NAME]
    if len(matching) > 1:
        raise RuntimeError('Duplicate AD federation providers; review manually.')
    provider_id = matching[0]['id'] if matching else None
    for username in USERS:
        existing = kc.call('get', 'users', '-q', 'username=' + username, '-q', 'exact=true')
        if any(not provider_id or u.get('federationLink') != provider_id for u in existing):
            raise RuntimeError('Username collision: ' + username + '. Existing account preserved.')

    provider = {
        'name': PROVIDER_NAME, 'providerId': 'ldap',
        'providerType': 'org.keycloak.storage.UserStorageProvider', 'parentId': realm_id,
        'config': config({
            'enabled': 'true', 'priority': '0', 'vendor': 'ad',
            'connectionUrl': 'ldaps://dc.ad.olaninc.test:636',
            'authType': 'simple', 'bindDn': 'CN=svc-keycloak,OU=ServiceAccounts,' + BASE_DN,
            'bindCredential': values['svc-keycloak'], 'useTruststoreSpi': 'always',
            'startTls': 'false', 'connectionPooling': 'true',
            'connectionTimeout': '5000', 'readTimeout': '10000',
            'usersDn': 'OU=DemoUsers,' + BASE_DN, 'searchScope': '2',
            'usernameLDAPAttribute': 'sAMAccountName', 'rdnLDAPAttribute': 'cn',
            'uuidLDAPAttribute': 'objectGUID', 'userObjectClasses': 'person, organizationalPerson, user',
            'editMode': 'READ_ONLY', 'importEnabled': 'true', 'syncRegistrations': 'false',
            'pagination': 'true', 'batchSizeForSync': '100',
            'fullSyncPeriod': '-1', 'changedSyncPeriod': '-1', 'cachePolicy': 'NO_CACHE',
            'trustEmail': 'false', 'allowKerberosAuthentication': 'false',
            'useKerberosForPasswordAuthentication': 'false',
        }),
    }
    if provider_id:
        kc.call('update', 'components/' + provider_id, payload=provider)
    else:
        kc.call('create', 'components', payload=provider)
        provider_id = next(p['id'] for p in kc.call('get', 'components', '-q', 'parent=' + realm_id)
                           if p['name'] == PROVIDER_NAME)

    roots = kc.call('get', 'groups', '-q', 'search=AD', '-q', 'exact=true')
    if not any(g['name'] == 'AD' for g in roots):
        kc.call('create', 'groups', payload={'name': 'AD'})
    mapper = {
        'name': 'catalog-ad-groups', 'providerId': 'group-ldap-mapper',
        'providerType': 'org.keycloak.storage.ldap.mappers.LDAPStorageMapper',
        'parentId': provider_id,
        'config': config({
            'groups.dn': 'OU=DemoGroups,' + BASE_DN,
            'group.name.ldap.attribute': 'cn', 'group.object.classes': 'group',
            'preserve.group.inheritance': 'false', 'membership.ldap.attribute': 'member',
            'membership.attribute.type': 'DN', 'membership.user.ldap.attribute': 'sAMAccountName',
            'mode': 'READ_ONLY', 'user.roles.retrieve.strategy': 'LOAD_GROUPS_BY_MEMBER_ATTRIBUTE',
            'memberof.ldap.attribute': 'memberOf', 'groups.path': '/AD',
            'drop.non.existing.groups.during.sync': 'false',
        }),
    }
    existing = kc.call('get', 'components', '-q', 'parent=' + provider_id)
    matching = [m for m in existing if m['name'] == mapper['name']]
    if matching:
        mapper_id = matching[0]['id']
        kc.call('update', 'components/' + mapper_id, payload=mapper)
    else:
        kc.call('create', 'components', payload=mapper)
        mapper_id = next(m['id'] for m in kc.call('get', 'components', '-q', 'parent=' + provider_id)
                         if m['name'] == mapper['name'])
    kc.call('create', f'user-storage/{provider_id}/mappers/{mapper_id}/sync',
            '-q', 'direction=fedToKeycloak')
    for group_name, role in [('catalog-users', 'users'), ('catalog-admins', 'admin')]:
        group = kc.call('get', 'group-by-path/AD/' + group_name)
        role_info = kc.call('get', 'roles/' + role)
        kc.call('create', 'groups/' + group['id'] + '/role-mappings/realm', payload=[role_info])
    result = kc.call('create', 'user-storage/' + provider_id + '/sync', '-q', 'action=triggerFullSync', '-o')
    if result.get('failed', 0):
        raise RuntimeError('AD synchronization reported failures; inspect Keycloak logs.')
    for username in USERS:
        users = kc.call('get', 'users', '-q', 'username=' + username, '-q', 'exact=true')
        if len(users) != 1 or users[0].get('federationLink') != provider_id:
            raise RuntimeError('Federated user not found: ' + username)
    print('Fédération LDAPS en lecture seule prête : 5 comptes et groupes /AD synchronisés.')


def main():
    values = credentials()
    print('Construction/démarrage de Samba AD (premier lancement : quelques minutes).', flush=True)
    subprocess.run(COMPOSE + ['--profile', 'ad', 'up', '--build', '-d', '--wait', 'ad'],
                   cwd=ROOT, check=True)
    ca = run(COMPOSE + ['exec', '-T', 'ad', 'cat', '/var/lib/samba/private/demo-tls/ca.crt'])
    if '-----BEGIN CERTIFICATE-----' not in ca:
        raise RuntimeError('Invalid Samba CA certificate.')
    changed = not CA_FILE.exists() or CA_FILE.read_text() != ca
    CA_FILE.parent.mkdir(exist_ok=True)
    CA_FILE.write_text(ca)
    CA_FILE.chmod(0o644)  # Public CA only; private keys stay in the Samba volume.
    args = COMPOSE + ['up', '-d', '--no-deps', '--wait']
    if changed:
        args += ['--force-recreate']
    subprocess.run(args + ['keycloak'], cwd=ROOT, check=True)
    kc = Keycloak()
    try:
        kc.login()
        configure_federation(kc, values)
    finally:
        kc.close()
    print('Identifiants dans .secrets/ad-credentials.json (0600), sans affichage des secrets.')
    print('Comptes locaux conservés. Aucun droit Keycloak/AD accordé à admin1. Vue/Rust inchangés.')


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, ValueError, KeyError, StopIteration, subprocess.CalledProcessError) as error:
        print('Configuration AD interrompue : ' + str(error), file=sys.stderr)
        sys.exit(1)
