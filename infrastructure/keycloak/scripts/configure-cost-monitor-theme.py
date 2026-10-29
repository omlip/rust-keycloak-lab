#!/usr/bin/env python3
"""Apply only the declared client Login theme overrides, preserving other settings."""
import json
import sys

from keycloak_admin import KeycloakAdmin, ROOT


def configure():
    realm = json.loads((ROOT / 'infrastructure/keycloak/olaninc-realm.json').read_text())
    with KeycloakAdmin() as kc:
        available = {theme['name'] for theme in kc.call('get', 'serverinfo')['themes']['login']}
        updates = []
        for name in ('cost-monitor-login', 'cost-monitor'):
            source = next(client for client in realm['clients'] if client['clientId'] == name)
            desired = source['attributes']['login_theme']
            if desired not in available:
                raise RuntimeError('Login theme not available; check the mounted themes directory.')
            clients = kc.call('get', 'clients', '-q', 'clientId=' + name)
            if len(clients) != 1:
                raise RuntimeError('Expected exactly one client: ' + name)
            attributes = dict(clients[0].get('attributes', {}))
            attributes['login_theme'] = desired
            updates.append((clients[0]['id'], attributes))
        for cid, attributes in updates:
            kc.call('update', 'clients/' + cid, payload={'attributes': attributes})
            current = kc.call('get', 'clients/' + cid)
            if current.get('attributes') != attributes:
                raise RuntimeError('Client theme verification failed.')
    print('Thème Login rouge appliqué aux deux clients Cost Monitor uniquement.')
    print('Thèmes du realm, autres attributs, secrets, comptes et sessions conservés.')


if __name__ == '__main__':
    try:
        configure()
    except (RuntimeError, OSError, ValueError, KeyError, StopIteration) as error:
        print('Configuration interrompue : ' + str(error), file=sys.stderr)
        sys.exit(1)
