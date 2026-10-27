#!/usr/bin/env python3
"""Apply only the declared Login, Account and Admin themes to the demo realm."""
import json
import sys

from keycloak_admin import KeycloakAdmin, ROOT


def configure():
    realm = json.loads((ROOT / 'infrastructure/keycloak/olaninc-realm.json').read_text())
    settings = {key: realm[key] for key in ('loginTheme', 'accountTheme', 'adminTheme')}
    with KeycloakAdmin() as kc:
        available = kc.call('get', 'serverinfo')['themes']
        for kind in ('login', 'account', 'admin'):
            if settings[kind + 'Theme'] not in {theme['name'] for theme in available[kind]}:
                raise RuntimeError('Theme not available for ' + kind + '; check the mounted themes directory.')
        kc.call('update', 'realms/olaninc', payload=settings)
        current = kc.call('get', 'realms/olaninc')
        if any(current.get(key) != value for key, value in settings.items()):
            raise RuntimeError('Realm theme verification failed.')
    print('Thème ni2 appliqué à Login, Account et Admin pour olaninc. Realm master inchangé.')
    print('Comptes, mots de passe, OTP, clients et sessions conservés. Recharger les consoles.')


if __name__ == '__main__':
    try:
        configure()
    except (RuntimeError, OSError, ValueError, KeyError) as error:
        print('Configuration interrompue : ' + str(error), file=sys.stderr)
        sys.exit(1)
