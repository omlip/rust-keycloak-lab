#!/usr/bin/env python3
"""Exercise real Authorization Code + PKCE logins and Rust authorization.

No password grant is enabled. Tokens/passwords are never printed. One temporary
catalog entry is created for admin1 and deleted in a finally block.
"""

import base64
import hashlib
import html
import http.cookiejar
import json
from pathlib import Path
import re
import secrets
import sys
import urllib.error
import urllib.parse
import urllib.request


ROOT = Path(__file__).resolve().parents[3]
KEYCLOAK = 'http://localhost:8081/realms/olaninc'
APP = 'http://localhost:8080'


def login(username, password):
    jar = http.cookiejar.CookieJar()
    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
    verifier = secrets.token_urlsafe(64)
    challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).decode().rstrip('=')
    state = secrets.token_urlsafe(24)
    url = KEYCLOAK + '/protocol/openid-connect/auth?' + urllib.parse.urlencode({
        'client_id': 'olaninc-spa', 'redirect_uri': APP + '/',
        'response_type': 'code', 'scope': 'openid', 'state': state,
        'code_challenge': challenge, 'code_challenge_method': 'S256',
    })
    with opener.open(url, timeout=20) as response:
        page = response.read().decode()
    # Browsers trust localhost for Secure cookies; urllib does not. This changes
    # only this test's in-memory jar, never server cookie or TLS configuration.
    for cookie in jar:
        cookie.secure = False
    form = re.search(r'<form\b[^>]*id="kc-form-login"[^>]*>', page)
    if not form:
        raise RuntimeError('Native login form not found.')
    action = html.unescape(re.search(r'action="([^"]+)"', form[0])[1])
    body = urllib.parse.urlencode({'username': username, 'password': password, 'credentialId': ''}).encode()
    with opener.open(action, body, timeout=20) as response:
        callback = urllib.parse.urlparse(response.geturl())
        params = urllib.parse.parse_qs(callback.query)
    if callback.scheme != 'http' or callback.netloc != 'localhost:8080' or callback.path != '/':
        raise RuntimeError('Login failed or unexpected required action for ' + username)
    if params.get('state') != [state] or 'code' not in params:
        raise RuntimeError('Invalid OIDC callback for ' + username)
    body = urllib.parse.urlencode({
        'grant_type': 'authorization_code', 'client_id': 'olaninc-spa',
        'redirect_uri': APP + '/', 'code': params['code'][0], 'code_verifier': verifier,
    }).encode()
    with opener.open(KEYCLOAK + '/protocol/openid-connect/token', body, timeout=20) as response:
        tokens = json.load(response)
    payload = tokens['access_token'].split('.')[1]
    claims = json.loads(base64.urlsafe_b64decode(payload + '=' * (-len(payload) % 4)))
    return tokens, claims


def api(method, path, token, payload=None):
    data = None if payload is None else json.dumps(payload).encode()
    req = urllib.request.Request(APP + path, data=data, method=method, headers={
        'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json',
    })
    try:
        with urllib.request.urlopen(req, timeout=20) as response:
            body = response.read()
            return response.status, json.loads(body) if body else None
    except urllib.error.HTTPError as error:
        return error.code, None


def logout(tokens):
    data = urllib.parse.urlencode({'client_id': 'olaninc-spa', 'refresh_token': tokens['refresh_token']}).encode()
    with urllib.request.urlopen(KEYCLOAK + '/protocol/openid-connect/logout', data, timeout=20) as response:
        assert response.status == 204


def main():
    credentials = json.loads((ROOT / '.secrets/ad-credentials.json').read_text())
    for username in ['user1', 'user2', 'user3', 'user4', 'admin1']:
        tokens, claims = login(username, credentials[username])
        try:
            roles = set(claims.get('realm_access', {}).get('roles', []))
            expected = 'admin' if username == 'admin1' else 'users'
            assert roles & {'users', 'admin', 'catalog-seed'} == {expected}
            assert not claims.get('resource_access', {}).get('realm-management')
            token = tokens['access_token']
            admin_request = urllib.request.Request('http://localhost:8081/admin/realms/olaninc/users?max=1',
                                                  headers={'Authorization': 'Bearer ' + token})
            try:
                with urllib.request.urlopen(admin_request, timeout=20):
                    raise AssertionError('Unexpected Keycloak administration access')
            except urllib.error.HTTPError as error:
                assert error.code == 403
            assert api('GET', '/api/catalog/services', token)[0] == 200
            assert api('POST', '/api/catalog/seed', token)[0] == 403
            payload = {'name': 'AD verification', 'category': 'compute',
                       'description': 'Temporary integration check', 'monthly_price_cents': 100}
            status, service = api('POST', '/api/catalog/services', token, payload)
            if username == 'admin1':
                assert status == 201, status
                try:
                    payload['name'] = 'AD verification updated'
                    assert api('PUT', '/api/catalog/services/' + str(service['id']), token, payload)[0] == 200
                finally:
                    assert api('DELETE', '/api/catalog/services/' + str(service['id']), token)[0] == 204
            else:
                assert status == 403, status
                for method in ('PUT', 'DELETE'):
                    assert api(method, '/api/catalog/services/0', token, payload)[0] == 403
            print(username + ': connexion AD, JWT et permissions API vérifiés ; seed refusé.')
        finally:
            logout(tokens)
    print('Vérification terminée. Service temporaire supprimé et sessions de test déconnectées.')


if __name__ == '__main__':
    try:
        main()
    except (AssertionError, RuntimeError, OSError, ValueError, KeyError) as error:
        # Do not print HTTP bodies or tokens on failure.
        print('Vérification AD échouée : ' + type(error).__name__ + ' ' + str(error), file=sys.stderr)
        sys.exit(1)
