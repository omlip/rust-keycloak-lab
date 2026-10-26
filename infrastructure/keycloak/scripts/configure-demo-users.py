#!/usr/bin/env python3
"""Configure local demo personas without deleting identities or OTP credentials.

Passwords are deliberately public and predictable for localhost demonstrations.
Existing passwords change ONLY with --reset-passwords. Technical users are excluded.
"""
import argparse
import json
import sys

from keycloak_admin import KeycloakAdmin, ROOT


USERNAMES = ('demo-user', 'demo-admin', 'demo-security', 'demo-bruteforce')


def configure(reset_passwords=False):
    realm = json.loads((ROOT / 'infrastructure/keycloak/olaninc-realm.json').read_text())
    specs = [next(user for user in realm['users'] if user['username'] == name)
             for name in USERNAMES]
    with KeycloakAdmin() as kc:
        existing = {}
        # Refuse identity collisions before modifying any account.
        for spec in specs:
            users = kc.call('get', 'users', '-q', 'username=' + spec['username'], '-q', 'exact=true')
            if len(users) > 1 or any(user.get('federationLink') for user in users):
                raise RuntimeError('Expected a unique local demo account: ' + spec['username'])
            existing[spec['username']] = users[0] if users else None
        for spec in specs:
            user = existing[spec['username']]
            if user is None:
                payload = {key: value for key, value in spec.items()
                           if key not in ('realmRoles', 'clientRoles')}
                kc.call('create', 'users', payload=payload)
                user = kc.call('get', 'users', '-q', 'username=' + spec['username'], '-q', 'exact=true')[0]
            elif reset_passwords:
                kc.call('update', 'users/' + user['id'] + '/reset-password',
                        payload=spec['credentials'][0])
                # Keep OTP and other required actions; remove only the initial password change.
                actions = [action for action in user.get('requiredActions', []) if action != 'UPDATE_PASSWORD']
                kc.call('update', 'users/' + user['id'], payload={'requiredActions': actions})
            roles = [kc.call('get', 'roles/' + name) for name in spec['realmRoles']]
            kc.call('create', 'users/' + user['id'] + '/role-mappings/realm', payload=roles)
            for client, names in spec.get('clientRoles', {}).items():
                client_id = kc.call('get', 'clients', '-q', 'clientId=' + client)[0]['id']
                roles = [kc.call('get', 'clients/' + client_id + '/roles/' + name) for name in names]
                kc.call('create', 'users/' + user['id'] + '/role-mappings/clients/' + client_id, payload=roles)
    print('Quatre comptes locaux de démonstration configurés ; identités et OTP conservés.')
    print('Mots de passe existants ' + ('réinitialisés explicitement.' if reset_passwords else 'conservés.'))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reset-passwords', action='store_true',
                        help='Replace passwords of the four local demo accounts with public demo values.')
    args = parser.parse_args()
    try:
        configure(args.reset_passwords)
    except (RuntimeError, OSError, ValueError, KeyError, IndexError, StopIteration) as error:
        print('Configuration interrompue : ' + str(error), file=sys.stderr)
        sys.exit(1)
