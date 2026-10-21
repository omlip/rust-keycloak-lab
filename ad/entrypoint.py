"""Local LDAP federation demo, not a production Windows domain replacement.

Provision only an empty volume. No privileged container or SYSVOL ACL setup:
this demo uses LDAPS, not Windows machine joining, GPOs or file sharing.
"""

import hashlib
import json
import logging
import os
from pathlib import Path
import socket
import subprocess

from samba.auth import system_session
from samba.dsdb import UF_DONT_EXPIRE_PASSWD
from samba.provision import provision


ROOT = Path('/var/lib/samba')
CONFIG = ROOT / 'etc/smb.conf'
TLS = ROOT / 'private/demo-tls'
BASE_DN = 'DC=ad,DC=olaninc,DC=test'
SECRETS = Path('/run/secrets/ad-credentials.json')
MARKER = ROOT / 'private/demo-ready'


def command(*args):
    result = subprocess.run(args, capture_output=True, text=True)
    if result.returncode:
        # Never dump command output: provisioning can contain credentials.
        raise RuntimeError(f'AD setup command failed: {args[0]}')


def certificates():
    TLS.mkdir(mode=0o700, parents=True, exist_ok=True)
    command('openssl', 'req', '-x509', '-newkey', 'rsa:3072', '-nodes',
            '-keyout', str(TLS / 'ca.key'), '-out', str(TLS / 'ca.crt'),
            '-days', '3650', '-subj', '/CN=Olaninc local demo AD CA',
            '-addext', 'basicConstraints=critical,CA:TRUE',
            '-addext', 'keyUsage=critical,keyCertSign,cRLSign')
    command('openssl', 'req', '-new', '-newkey', 'rsa:3072', '-nodes',
            '-keyout', str(TLS / 'server.key'), '-out', str(TLS / 'server.csr'),
            '-subj', '/CN=dc.ad.olaninc.test')
    extensions = TLS / 'server.ext'
    extensions.write_text('subjectAltName=DNS:dc.ad.olaninc.test\n'
                          'basicConstraints=critical,CA:FALSE\n'
                          'keyUsage=critical,digitalSignature,keyEncipherment\n'
                          'extendedKeyUsage=serverAuth\n')
    command('openssl', 'x509', '-req', '-in', str(TLS / 'server.csr'),
            '-CA', str(TLS / 'ca.crt'), '-CAkey', str(TLS / 'ca.key'),
            '-CAcreateserial', '-out', str(TLS / 'server.crt'),
            '-days', '825', '-extfile', str(extensions))
    for path in TLS.glob('*.key'):
        path.chmod(0o600)


def initialize(credentials):
    if CONFIG.exists() or (ROOT / 'private/sam.ldb').exists():
        raise RuntimeError('Partial AD volume detected; refusing to overwrite it.')
    logger = logging.getLogger('ad-provision')
    logger.addHandler(logging.StreamHandler())
    logger.setLevel(logging.WARNING)
    result = provision(
        logger, system_session(), targetdir=str(ROOT),
        realm='AD.OLANINC.TEST', domain='OLANINC', hostname='dc',
        hostip=socket.gethostbyname(socket.gethostname()),
        adminpass=credentials['Administrator'],
        serverrole='active directory domain controller',
        dns_backend='SAMBA_INTERNAL', use_rfc2307=True,
        skip_sysvolacl=True,
    )
    db = result.samdb
    for ou in ('DemoUsers', 'DemoGroups', 'ServiceAccounts'):
        db.create_ou(f'OU={ou},{BASE_DN}')
    for username in ('user1', 'user2', 'user3', 'user4', 'admin1', 'svc-keycloak'):
        ou = 'ServiceAccounts' if username == 'svc-keycloak' else 'DemoUsers'
        db.newuser(username, credentials[username], useusernameascn=True,
                   userou=f'OU={ou}', givenname=username, surname='Demo',
                   mailaddress=f'{username}@ad.olaninc.test')
        db.toggle_userAccountFlags(f'(sAMAccountName={username})', UF_DONT_EXPIRE_PASSWD)
    for group, members in (
        ('catalog-users', ['user1', 'user2', 'user3', 'user4']),
        ('catalog-admins', ['admin1']),
    ):
        db.newgroup(group, groupou='OU=DemoGroups')
        db.add_remove_group_members(group, members)
    certificates()
    # Add TLS parameters to the global section, not the final share section.
    content = CONFIG.read_text()
    content = content.replace('[global]', '[global]\n'
        f'\ttls enabled = yes\n\ttls keyfile = {TLS}/server.key\n'
        f'\ttls certfile = {TLS}/server.crt\n\ttls cafile = {TLS}/ca.crt\n'
        '\tldap server require strong auth = yes\n', 1)
    CONFIG.write_text(content)


def main():
    os.umask(0o077)
    data = SECRETS.read_bytes()
    credentials = json.loads(data)
    digest = hashlib.sha256(data).hexdigest()
    if not MARKER.exists():
        initialize(credentials)
        MARKER.write_text(digest)
    elif MARKER.read_text() != digest:
        raise RuntimeError('AD credentials file differs from the persisted volume. '
                           'Restore the original file; passwords are not reset automatically.')
    Path('/run/keycloak-bind.password').write_text(credentials['svc-keycloak'])
    print('Samba AD ready: persisted domain, LDAPS, five demo users.', flush=True)
    os.execvp('samba', ['samba', '--interactive', '--no-process-group',
                       '--configfile=' + str(CONFIG), '--debug-stdout'])


if __name__ == '__main__':
    main()
