#!/usr/bin/env python3
"""Migrate only Cost Monitor's login client to SPA PKCE; preserve the chosen theme."""
import json
import sys

from keycloak_admin import KeycloakAdmin, ROOT


def configure():
    realm = json.loads((ROOT / 'infrastructure/keycloak/olaninc-realm.json').read_text())
    source = next(c for c in realm['clients'] if c['clientId'] == 'cost-monitor-login')
    fields = ('publicClient', 'standardFlowEnabled', 'implicitFlowEnabled',
              'directAccessGrantsEnabled', 'serviceAccountsEnabled', 'redirectUris', 'webOrigins')
    with KeycloakAdmin() as kc:
        clients = kc.call('get', 'clients', '-q', 'clientId=cost-monitor-login')
        if len(clients) != 1:
            raise RuntimeError('Expected the existing cost-monitor-login client.')
        current = clients[0]
        attributes = dict(current.get('attributes', {}))
        for key in ('pkce.code.challenge.method', 'post.logout.redirect.uris'):
            attributes[key] = source['attributes'][key]
        mappers = {m['name']: m for m in current.get('protocolMappers', [])}
        for mapper in source['protocolMappers']:
            mappers[mapper['name']] = {**mappers.get(mapper['name'], {}), **mapper}
        payload = {key: source[key] for key in fields}
        payload.update(attributes=attributes, protocolMappers=list(mappers.values()))
        kc.call('update', 'clients/' + current['id'], payload=payload)
        result = kc.call('get', 'clients/' + current['id'])
        scalar_fields = tuple(key for key in fields if key not in ('redirectUris', 'webOrigins'))
        if (any(result.get(key) != source[key] for key in scalar_fields)
                or any(set(result.get(key, [])) != set(source[key]) for key in ('redirectUris', 'webOrigins'))
                or result.get('attributes') != attributes):
            raise RuntimeError('SPA client verification failed.')
        if not any(m['name'] == 'cost-monitor-api-audience' and m.get('config') == source['protocolMappers'][0]['config']
                   for m in result.get('protocolMappers', [])):
            raise RuntimeError('API audience verification failed.')
    print('Client Cost Monitor migré vers une SPA publique PKCE S256, avec audience cost-monitor-api.')
    print('Thème choisi, délégation catalogue, comptes et sessions conservés. Aucun secret SPA requis.')


if __name__ == '__main__':
    try:
        configure()
    except (RuntimeError, OSError, ValueError, KeyError, StopIteration) as error:
        print('Migration interrompue : ' + str(error), file=sys.stderr)
        sys.exit(1)
