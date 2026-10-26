# Démonstration Active Directory avec Samba

Cette option ajoute un **annuaire compatible Active Directory**, pas un serveur
Microsoft Windows. Samba AD DC tourne dans une image construite à partir de Debian
Trixie et de ses paquets Samba, sur ARM64 comme sur AMD64. Aucun conteneur
privilégié, réseau hôte ou port d’annuaire publié n’est nécessaire.

## Démarrage

Démarrer d’abord l’application normalement, puis exécuter depuis la racine :

```sh
docker compose up --build -d --wait
python3 infrastructure/active-directory/scripts/configure-ad.py
docker compose --profile ad ps
```

Le script construit et démarre le service optionnel `ad`, récupère son certificat
d’autorité **public**, recharge Keycloak si nécessaire et configure le fournisseur
LDAP natif. Il ne redémarre pas Rust, ne vide pas le catalogue et ne supprime
aucun compte ou volume existant. Les identifiants d’administration Keycloak
alternatifs se fournissent comme pour `configure-catalog-loader.py`, via
`KEYCLOAK_ADMIN_USER` et `KEYCLOAK_ADMIN_PASSWORD`.

L’annuaire est provisionné **une seule fois**, dans le volume `samba-data`. Les
relances ne changent ni les mots de passe, ni les groupes AD existants. Une
initialisation partielle provoque un arrêt plutôt qu’un écrasement des données.
Si le fichier des secrets est perdu alors que le domaine existe, restaurer le
fichier original : aucun renouvellement automatique des mots de passe n’est fait.

## Comptes et rôles

| Compte AD | Groupe AD | Rôle applicatif |
| --- | --- | --- |
| `user1` | `catalog-users` | `users` |
| `user2` | `catalog-users` | `users` |
| `user3` | `catalog-users` | `users` |
| `user4` | `catalog-users` | `users` |
| `admin1` | `catalog-admins` | `admin` |

Les mots de passe humains suivent la convention publique
**`<identifiant>-12345!`**, par exemple `user1-12345!`.
Ils sont prévisibles et **réservés à la démo localhost**. Ils n’expirent pas
automatiquement. Les secrets de `Administrator` et `svc-keycloak` restent
aléatoires et ne suivent pas cette convention.

Toutes ces valeurs sont conservées dans **`.secrets/ad-credentials.json`**,
fichier ignoré par Git, permissions `0600` dans un dossier `0700`.
Ne jamais partager ou versionner ce fichier : il contient aussi les secrets
techniques, qui ne sont pas des identifiants publics de démonstration.

Sur une installation existante, la configuration ordinaire conserve les anciens
mots de passe. Pour adopter explicitement la convention, avec Samba démarré :

```sh
python3 infrastructure/active-directory/scripts/set-demo-passwords.py
```

Ce script modifie uniquement les cinq mots de passe humains, sans recréer les
comptes, modifier leurs groupes ou renouveler les secrets techniques. Il met
également à jour le marqueur de cohérence du volume et le fichier monté, en
conservant son inode. Ne pas modifier ce fichier manuellement : le prochain
démarrage refuserait un désaccord avec le marqueur.

Deux comptes techniques supplémentaires existent dans l’annuaire :

- `Administrator` : administration du domaine Samba, pas de connexion à la SPA ;
- `svc-keycloak` : compte ordinaire de lecture LDAP, sans délégation administrative.

Ces comptes se trouvent hors de l’OU synchronisée par Keycloak. `admin1` n’est
**ni administrateur AD, ni administrateur Keycloak**, et ne peut pas lancer le
seed. Le rôle applicatif `admin` ne donne que les droits de gestion du catalogue.
L’interface actuelle affiche un lien d’administration Keycloak pour le rôle
applicatif `admin` ; ce lien n’accorde aucun droit : `admin1` n’a pas accès à
l’administration du realm. `demo-admin` conserve son accès administratif distinct.

Les comptes locaux `demo-user`, `demo-admin`, `demo-security` et
`demo-bruteforce` restent indépendants de l’annuaire ; la configuration AD
ordinaire ne change ni leurs mots de passe, ni leurs OTP.

## Paramètres de fédération

| Paramètre | Valeur |
| --- | --- |
| Domaine DNS / realm AD | `ad.olaninc.test` / `AD.OLANINC.TEST` |
| Domaine court | `OLANINC` |
| Contrôleur interne Docker | `dc.ad.olaninc.test` |
| Connexion | `ldaps://dc.ad.olaninc.test:636` |
| Base utilisateurs | `OU=DemoUsers,DC=ad,DC=olaninc,DC=test` |
| Base groupes | `OU=DemoGroups,DC=ad,DC=olaninc,DC=test` |
| Compte de liaison | `CN=svc-keycloak,OU=ServiceAccounts,DC=ad,DC=olaninc,DC=test` |
| Fournisseur Keycloak | `olaninc-samba-ad`, vendor Active Directory |
| Identifiant utilisateur | `sAMAccountName`, UUID `objectGUID` |
| Mode | `READ_ONLY`, import des profils activé, cache désactivé |
| Groupes Keycloak | `/AD/catalog-users` et `/AD/catalog-admins` |

La clé privée de l’autorité locale et le certificat serveur restent dans le
volume Samba. Seule l’autorité publique est copiée vers
`infrastructure/keycloak/truststores/ad-ca.crt` (ignorée par Git), montée en lecture seule et
chargée par le truststore système Keycloak. Le nom DNS est vérifié et aucun
mode TLS permissif n’est activé. Le certificat serveur expire après 825 jours :
son renouvellement est une opération de maintenance, non automatisée ici.

Les mots de passe AD ne sont pas copiés dans la base Keycloak : Keycloak effectue
leur vérification auprès de Samba. Les profils importés et les credentials locaux
éventuels (par exemple OTP) restent gérés par Keycloak. Les formulaires de
changement/réinitialisation de mot de passe et d’édition des attributs LDAP ne
permettent pas d’écrire dans cet annuaire **en lecture seule**. Les scénarios de
modification de mot de passe/profil restent à démontrer avec les comptes locaux.

## Scénario de présentation

1. Dans Keycloak, montrer **User federation → olaninc-samba-ad** : annuaire AD,
   liaison LDAPS et mode lecture seule.
2. Montrer les groupes AD puis leurs correspondances dans **Groups → AD** :
   `catalog-users` reçoit `users`, `catalog-admins` reçoit `admin`.
3. Sur <http://localhost:8080>, se connecter avec `user1`, puis avec `admin1`
   dans une autre session privée : consultation bleue contre gestion prune.
4. Montrer qu’aucun compte n’a été créé manuellement dans Keycloak ; les cinq
   profils ont un lien de fédération et leurs mots de passe restent dans AD.
5. Vérifier que `admin1` peut gérer le catalogue mais pas l’administration IAM.
6. Montrer que les comptes locaux fonctionnent encore en parallèle.

Pour changer un groupe AD sans toucher au mot de passe :

```sh
docker compose exec -T ad samba-tool group addmembers catalog-admins user4 \
  --configfile=/var/lib/samba/etc/smb.conf
```

Reconnecter `user4` pour obtenir un **nouveau jeton**. Il conserve `users` et
reçoit désormais aussi `admin`. Les JWT déjà émis ne changent pas. Rétablir
ensuite la configuration initiale :

```sh
docker compose exec -T ad samba-tool group removemembers catalog-admins user4 \
  --configfile=/var/lib/samba/etc/smb.conf
```

Les groupes existants sont interrogés en lecture seule avec le mode du mapper
`READ_ONLY`. Pour importer de nouveaux utilisateurs/groupes ou rejouer la
configuration et la synchronisation complète, relancer `configure-ad.py` ou
utiliser les boutons de synchronisation natifs. Aucune synchronisation
périodique n’est activée pour cette petite démo.

## Vérification automatisée

```sh
python3 infrastructure/active-directory/scripts/verify-ad.py
python3 -m unittest discover -s infrastructure/active-directory/scripts/tests -v
```

Le script réalise les cinq connexions via **Authorization Code + PKCE** (aucun
password grant), vérifie les rôles dans les JWT et les réponses de l’API Rust.
Les quatre utilisateurs ne peuvent pas écrire, `admin1` peut créer/modifier/
supprimer, aucun compte ne peut lancer le seed. Un service temporaire est créé
puis supprimé ; les sessions de test sont déconnectées. Secrets et jetons ne
sont pas affichés. Cette vérification suppose les groupes initiaux ci-dessus.
Les tests unitaires vérifient aussi la protection et la conservation des secrets,
le refus d’écraser des liens symboliques ou de régénérer des mots de passe pour
un domaine existant, et le refus d’une collision avec un compte local.

## Arrêt, redémarrage et limites

```sh
docker compose --profile ad down
docker compose --profile ad up -d --wait
```

Les données persistent sans `--volumes`. Une fois la fédération activée, penser
à démarrer le profil `ad` ; un simple `docker compose up` ne démarre pas un
annuaire arrêté. Les connexions AD nécessitent l’annuaire disponible.

**`docker compose --profile ad down --volumes` efface aussi le domaine Samba**, en
plus de Keycloak et Mailpit. Pour une reconstruction volontaire, le script peut
réutiliser les secrets locaux et synchronise la nouvelle autorité publique.
Les identifiants AD internes changent après reprovisionnement du domaine : ne
pas recréer Samba seul en espérant conserver les UUID des utilisateurs déjà
importés dans un Keycloak existant.

Le DNS interne Samba est provisionné, mais les applications utilisent l’alias
DNS Docker stable. Pas de modification DNS du Mac, de jonction de machines
Windows, de GPO, de partage SMB, de réplication multi-DC ou de SSO Kerberos.
Les ACL SYSVOL ne sont pas provisionnées, ce qui évite des privilèges système
inutiles pour **ce seul scénario LDAPS**. Ce montage n’est pas un contrôleur de
domaine de production et ne valide pas une intégration à Microsoft AD lui-même.

Références : [Samba AD](https://wiki.samba.org/index.php/Setting_up_Samba_as_an_Active_Directory_Domain_Controller),
[fédération LDAP/AD Keycloak](https://www.keycloak.org/docs/26.8.1/server_admin/#_ldap),
[certificats de confiance Keycloak](https://www.keycloak.org/server/keycloak-truststore).
