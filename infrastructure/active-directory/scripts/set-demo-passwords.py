#!/usr/bin/env python3
"""Explicitly set the five human AD demo passwords; never rotate technical secrets.

Run only for the localhost demo. Ordinary configure-ad.py replays do not reset passwords.
The Samba marker and the bind-mounted credentials file must remain synchronized.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[3]
SECRET_FILE = ROOT / '.secrets/ad-credentials.json'
USERS = ('user1', 'user2', 'user3', 'user4', 'admin1')
ACCOUNTS = {'Administrator', 'svc-keycloak', *USERS}
COMPOSE = ['docker', 'compose', '-f', str(ROOT / 'compose.yaml')]

REMOTE = '''
import json, sys
from pathlib import Path
import ldb
from samba.auth import system_session
from samba.param import LoadParm
from samba.samdb import SamDB

data = json.load(sys.stdin)
marker = Path('/var/lib/samba/private/demo-ready')
if marker.read_text() != data['expected_digest']:
    raise RuntimeError('Samba marker mismatch; no passwords changed')
if not set(data['passwords']).issubset({'user1', 'user2', 'user3', 'user4', 'admin1'}):
    raise RuntimeError('Only human demo users may be updated')
lp = LoadParm()
lp.load('/var/lib/samba/etc/smb.conf')
db = SamDB(url='/var/lib/samba/private/sam.ldb', session_info=system_session(), lp=lp)
db.transaction_start()
try:
    for name, password in data['passwords'].items():
        users = db.search(base=db.domain_dn(), scope=ldb.SCOPE_SUBTREE,
                          expression='(sAMAccountName=' + name + ')', attrs=[])
        if len(users) != 1:
            raise RuntimeError('Expected one demo user')
        message = ldb.Message()
        message.dn = users[0].dn
        message['unicodePwd'] = ldb.MessageElement(
            ('"' + password + '"').encode('utf-16-le'), ldb.FLAG_MOD_REPLACE, 'unicodePwd')
        db.modify(message)
    db.transaction_commit()
except BaseException:
    db.transaction_cancel()
    raise
marker.write_text(data['next_digest'])
'''


def update_ad(passwords, expected_digest, next_digest):
    result = subprocess.run(COMPOSE + ['exec', '-T', 'ad', 'python3', '-c', REMOTE],
                            cwd=ROOT, text=True, capture_output=True,
                            input=json.dumps({'passwords': passwords, 'expected_digest': expected_digest,
                                              'next_digest': next_digest}))
    if result.returncode:
        raise RuntimeError('AD password maintenance failed; no command output is displayed.')


def configure():
    if SECRET_FILE.parent.is_symlink() or SECRET_FILE.is_symlink():
        raise RuntimeError('Secret paths must not be symbolic links.')
    original = SECRET_FILE.read_bytes()
    values = json.loads(original)
    if set(values) != ACCOUNTS or any(not isinstance(value, str) or len(value) < (12 if name in USERS else 16)
                                    for name, value in values.items()):
        raise RuntimeError('Invalid AD credentials file; existing data preserved.')
    changes = {name: name + '-12345!' for name in USERS if values[name] != name + '-12345!'}
    old_digest = hashlib.sha256(original).hexdigest()
    if not changes:
        # Still verify the marker without rewriting passwords or triggering history checks.
        update_ad({}, old_digest, old_digest)
        print('Mots de passe AD de démonstration déjà configurés ; marqueur vérifié.')
        return
    updated = dict(values, **changes)
    content = (json.dumps(updated, indent=2) + '\n').encode()
    new_digest = hashlib.sha256(content).hexdigest()
    # Keep the inode: Docker bind-mounts this file into the running Samba container.
    with SECRET_FILE.open('r+b') as stream:
        if stream.read() != original:
            raise RuntimeError('Credentials changed concurrently; no passwords changed.')
        os.fchmod(stream.fileno(), 0o600)
        update_ad(changes, old_digest, new_digest)
        try:
            stream.seek(0)
            stream.write(content)
            stream.truncate()
            stream.flush()
            os.fsync(stream.fileno())
        except OSError:
            update_ad({name: values[name] for name in changes}, new_digest, old_digest)
            stream.seek(0)
            stream.write(original)
            stream.truncate()
            stream.flush()
            raise
    print('Cinq mots de passe humains AD configurés ; identités, groupes et secrets techniques conservés.')


if __name__ == '__main__':
    try:
        configure()
    except (RuntimeError, OSError, ValueError, KeyError) as error:
        print('Configuration interrompue : ' + str(error), file=sys.stderr)
        sys.exit(1)
