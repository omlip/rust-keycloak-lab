#!/usr/bin/env python3
"""Configure independent Cost Monitor login and catalogue-delegation clients."""
import json
import os
from pathlib import Path
import sys

from keycloak_admin import KeycloakAdmin, ROOT


def configure():
    realm = json.loads((ROOT / 'infrastructure/keycloak/olaninc-realm.json').read_text())
    with KeycloakAdmin() as kc:
        scopes = {s['name']: s['id'] for s in kc.call('get', 'client-scopes')}
        for scope in realm['clientScopes']:
            name = scope['name']
            if name in scopes:
                kc.call('update', 'client-scopes/' + scopes[name], payload=scope)
            else:
                kc.call('create', 'client-scopes', payload=scope)
        scopes = {s['name']: s['id'] for s in kc.call('get', 'client-scopes')}
        managed = {}
        for name in ['cost-monitor-login', 'cost-monitor']:
            client = next(c for c in realm['clients'] if c['clientId'] == name)
            clients = kc.call('get', 'clients', '-q', 'clientId=' + name)
            if clients:
                cid = clients[0]['id']
                kc.call('update', 'clients/' + cid, payload=client)
            else:
                kc.call('create', 'clients', payload=client)
                cid = kc.call('get', 'clients', '-q', 'clientId=' + name)[0]['id']
            managed[name] = cid
        cid = managed['cost-monitor']
        roles = [kc.call('get', 'roles/' + role) for role in ['users', 'admin', 'offline_access']]
        kc.call('create', f'clients/{cid}/scope-mappings/realm', payload=roles)
        for client_name in ['olaninc-spa', 'catalog-loader', 'cost-monitor', 'cost-monitor-login']:
            source = next(c for c in realm['clients'] if c['clientId'] == client_name)
            target = kc.call('get', 'clients', '-q', 'clientId=' + client_name)[0]['id']
            if client_name in managed:
                # Keep authentication and delegation grants disjoint, including
                # on replay after a manual configuration change.
                for kind, desired in [('default', source['defaultClientScopes']), ('optional', source['optionalClientScopes'])]:
                    for existing in kc.call('get', f'clients/{target}/{kind}-client-scopes'):
                        if existing['name'] not in desired:
                            kc.call('delete', f'clients/{target}/{kind}-client-scopes/{existing["id"]}')
            for name in source['defaultClientScopes']:
                kc.call('update', f'clients/{target}/default-client-scopes/{scopes[name]}')
            for name in source['optionalClientScopes']:
                kc.call('update', f'clients/{target}/optional-client-scopes/{scopes[name]}')
        directory = ROOT / '.secrets'
        if directory.is_symlink() or any((directory / (name + '.secret')).is_symlink() for name in managed):
            raise RuntimeError('Secret path must not be a symbolic link.')
        directory.mkdir(mode=0o700, exist_ok=True)
        directory.chmod(0o700)
        for name, cid in managed.items():
            value = kc.call('get', f'clients/{cid}/client-secret')['value']
            fd = os.open(directory / (name + '.secret'), os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
            with os.fdopen(fd, 'w') as stream:
                os.fchmod(stream.fileno(), 0o600)
                stream.write(value)
    print('Cost Monitor configuré : connexion applicative sans accès API et délégation catalogue offline séparée.')
    print('Deux secrets synchronisés dans .secrets/ (0600), sans rotation ni affichage.')
    print('Reconnecter la SPA pour recevoir ses nouveaux scopes. Comptes, sessions et mots de passe conservés.')


if __name__ == '__main__':
    try:
        configure()
    except (RuntimeError, OSError, ValueError, KeyError, IndexError, StopIteration) as error:
        print('Configuration interrompue : ' + str(error), file=sys.stderr)
        sys.exit(1)
