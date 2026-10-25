import os
import subprocess

result = subprocess.run([
    'ldapwhoami', '-x', '-H', 'ldaps://dc.ad.olaninc.test:636',
    '-D', 'CN=svc-keycloak,OU=ServiceAccounts,DC=ad,DC=olaninc,DC=test',
    '-y', '/run/keycloak-bind.password',
], env={**os.environ, 'LDAPTLS_CACERT': '/var/lib/samba/private/demo-tls/ca.crt',
        'LDAPTLS_REQCERT': 'demand'}, capture_output=True, timeout=8)
raise SystemExit(result.returncode)
