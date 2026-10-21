#!/usr/bin/env python3
"""Apply the dedicated service account using Keycloak's official Admin CLI.

Only catalog-loader and catalog-seed are configured. Existing human accounts,
passwords, realm settings and client secrets are not reset. Python 3 + Docker
Compose are the only host requirements; no third-party Python package is used.
"""

import json
import os
from pathlib import Path
import subprocess
import sys
import uuid


ROOT = Path(__file__).resolve().parents[1]
COMPOSE = ["docker", "compose", "-f", str(ROOT / "compose.yaml")]
CLIENT_ID = "catalog-loader"
ROLE = "catalog-seed"
SECRET_FILE = ROOT / ".secrets" / "catalog-loader.secret"


def run_container(arguments, *, data=None):
    result = subprocess.run(
        COMPOSE + ["exec", "-T", "keycloak"] + arguments,
        cwd=ROOT, input=data, text=True, capture_output=True, check=False,
    )
    if result.returncode:
        # Do not print stdout: it can contain client secrets or credentials.
        raise RuntimeError(result.stderr.strip() or "Échec de la commande Keycloak")
    return result.stdout


def save_secret(secret):
    directory = SECRET_FILE.parent
    if directory.is_symlink() or SECRET_FILE.is_symlink():
        raise RuntimeError("Le dossier/fichier de secret ne doit pas être un lien symbolique")
    directory.mkdir(mode=0o700, exist_ok=True)
    directory.chmod(0o700)
    fd = os.open(SECRET_FILE, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as stream:
        os.fchmod(stream.fileno(), 0o600)
        stream.write(secret)


def configure():
    realm = json.loads((ROOT / "keycloak" / "olaninc-realm.json").read_text())
    target_realm = realm["realm"]
    client = next(client for client in realm["clients"] if client["clientId"] == CLIENT_ID)
    role = next(role for role in realm["roles"]["realm"] if role["name"] == ROLE)
    config = f"/tmp/catalog-loader-kcadm-{uuid.uuid4().hex}.json"

    def kcadm(*arguments, payload=None):
        options = list(arguments) + ["--config", config, "-r", target_realm]
        if payload is not None:
            options += ["-f", "-"]
        return run_container(
            ["/opt/keycloak/bin/kcadm.sh"] + options,
            data=None if payload is None else json.dumps(payload),
        )

    try:
        # Credentials are expanded inside the container, not embedded in this file
        # or passed as literal secrets in the host command line.
        login = subprocess.run(
            COMPOSE + ["exec", "-T", "-e", "KEYCLOAK_ADMIN_USER", "-e", "KEYCLOAK_ADMIN_PASSWORD", "keycloak", "sh", "-c",
                       'umask 077; exec /opt/keycloak/bin/kcadm.sh config credentials '
                       '--config "$1" --server http://localhost:8080 --realm master '
                       '--user "${KEYCLOAK_ADMIN_USER:-$KC_BOOTSTRAP_ADMIN_USERNAME}" '
                       '--password "${KEYCLOAK_ADMIN_PASSWORD:-$KC_BOOTSTRAP_ADMIN_PASSWORD}"',
                       "sh", config],
            cwd=ROOT, text=True, capture_output=True, check=False,
        )
        if login.returncode:
            raise RuntimeError(
                "Connexion d’administration impossible. Aucun compte n’a été réinitialisé. "
                "Fournir KEYCLOAK_ADMIN_USER et KEYCLOAK_ADMIN_PASSWORD si les identifiants initiaux ont changé."
            )

        roles = json.loads(kcadm("get", "roles"))
        if not any(existing["name"] == ROLE for existing in roles):
            kcadm("create", "roles", payload=role)
        print(f"Rôle {ROLE} prêt.")

        clients = json.loads(kcadm("get", "clients", "-q", f"clientId={CLIENT_ID}"))
        if clients:
            client_id = clients[0]["id"]
            kcadm("update", f"clients/{client_id}", payload=client)
        else:
            # No hardcoded secret: Keycloak generates it for this confidential client.
            kcadm("create", "clients", payload=client)
            client_id = json.loads(kcadm("get", "clients", "-q", f"clientId={CLIENT_ID}"))[0]["id"]

        # The client representation and the dedicated-scope mappers are shared with
        # the initial realm import. The role scope mapping is a separate REST resource.
        seed_role = json.loads(kcadm("get", f"roles/{ROLE}"))
        kcadm("create", f"clients/{client_id}/scope-mappings/realm", payload=[seed_role])
        account = json.loads(kcadm("get", f"clients/{client_id}/service-account-user"))
        kcadm("add-roles", "--uid", account["id"], "--rolename", ROLE)

        credentials = json.loads(kcadm("get", f"clients/{client_id}/client-secret"))
        secret = credentials.get("value")
        if not secret:
            raise RuntimeError("Keycloak n’a pas fourni le secret du client")
        save_secret(secret)
        print(f"Client {CLIENT_ID} configuré : audience olaninc-api, rôle {ROLE}.")
        print(f"Secret enregistré dans {SECRET_FILE.relative_to(ROOT)} (permissions 0600, ignoré par Git).")
        print("Aucun secret affiché. Comptes humains et mots de passe conservés.")
    finally:
        run_container(["rm", "-f", config])


if __name__ == "__main__":
    try:
        configure()
    except (RuntimeError, OSError, ValueError, KeyError, IndexError) as error:
        print(f"Configuration interrompue : {error}", file=sys.stderr)
        sys.exit(1)
