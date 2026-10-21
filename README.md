# Olaninc — démonstration IAM

Une SPA **Vue / TypeScript**, une API **Rust / Axum** et **Keycloak** pour démontrer l’authentification OIDC, les rôles et les opérations IAM natives. **Mailpit** capture tous les e-mails. Le thème Login Keycloak reprend la palette Ni² par CSS, sans remplacement des formulaires natifs. Aucune base applicative et aucun endpoint d’administration IAM dans Rust.

### Thème Login Ni²

`keycloak/themes/ni2/login` hérite de `keycloak.v2` (Keycloak 26.8 / PatternFly 5). Il conserve les templates, scripts, erreurs et parcours natifs ; seuls les styles changent : bleu nuit, boutons bleus, fond gris clair et accent orange. Le nom du realm reste inchangé. Aucun thème Account, Administration ou e-mail n’est modifié.

Le symbole de démonstration représente deux baies de serveurs bleu nuit avec des accents orange ; ce n’est pas le logo officiel Ni². Le même SVG est utilisé comme logo et favicon dans Vue et le thème Login. Les fichiers `frontend/public/logo.svg` et `keycloak/themes/ni2/login/resources/img/logo.svg` doivent rester identiques. Le logo est décoratif à côté des titres/libellés existants, sans ajouter de texte redondant aux lecteurs d’écran.

Le dossier est monté en lecture seule dans `/opt/keycloak/themes/ni2`. Les caches de thèmes sont désactivés dans cette configuration locale pour faciliter les ajustements CSS ; ce réglage n’est pas destiné à la production. Le thème couvre la connexion, les mots de passe oubliés/réinitialisés ou imposés, ainsi que la saisie/configuration OTP des parcours Login. Les écrans propres à la console Account restent natifs.

Sur une installation vierge, l’import sélectionne automatiquement `ni2`. Sur un realm existant, choisir **Realm settings → Themes → Login theme → ni2 → Save**, sans supprimer les volumes. Pour revenir au thème natif, sélectionner `keycloak.v2` ; les comptes et sessions ne sont pas réinitialisés. Après un changement de version Keycloak, vérifier les styles, focus, erreurs, écrans mobiles et le QR code OTP.

Un **catalogue fictif de services de datacenter** illustre les permissions : `users` et `admin` le consultent ; seul `admin` peut ajouter, modifier et supprimer les services. Le compte technique `catalog-loader`, avec le rôle dédié `catalog-seed`, peut uniquement charger les cinq services prédéfinis. Le catalogue est vide au démarrage et stocké uniquement en mémoire. Son cœur applicatif suit une architecture hexagonale légère.

> **Local uniquement.** HTTP, base Keycloak embarquée et identifiants de démonstration connus : ce projet n’est pas une configuration de production. Ne pas utiliser de véritables mots de passe ou données personnelles.

## Démarrer

Prérequis : Docker avec Docker Compose récent, ports `8080`, `8081` et `8025` disponibles. Node et Rust ne sont pas nécessaires pour le démarrage Docker. Le premier build télécharge et compile les dépendances ; prévoir quelques minutes.

```sh
docker compose up --build -d --wait
docker compose ps
```

| Service | URL navigateur |
| --- | --- |
| Application | http://localhost:8080 |
| Keycloak | http://localhost:8081 |
| Console de compte | http://localhost:8081/realms/olaninc/account/ |
| Administration du realm | http://localhost:8081/admin/olaninc/console/ |
| Mailpit | http://localhost:8025 |

Utiliser **`localhost`**, pas `127.0.0.1`, dans le navigateur : issuer, origines et retours OIDC sont configurés pour ce nom. L’API n’a pas de port publié ; elle est accessible via `http://localhost:8080/api/...`. Le SMTP n’est accessible que dans le réseau Docker.

### Comptes de démonstration

| Compte | Mot de passe initial **temporaire** | Rôles applicatifs | Permissions Keycloak |
| --- | --- | --- | --- |
| `demo-user` | `User-local-only-2026!` | `users` | Console de compte personnelle |
| `demo-admin` | `Admin-local-only-2026!` | `admin` | Administration complète du realm `olaninc` |

**À la première connexion, chacun doit choisir un nouveau mot de passe d’au moins 12 caractères sur l’écran natif Keycloak.** Le mot de passe initial ne fonctionne plus ensuite. Les adresses `user@olaninc.test` et `admin@olaninc.test` sont fictives ; les deux boîtes se consultent dans la même interface Mailpit.

Le compte technique initial est distinct : `bootstrap-admin` / `Bootstrap-local-only-2026!`, dans le realm **master**, console http://localhost:8081/admin/master/console/. Ne pas l’utiliser dans la SPA. Ces identifiants sont uniquement destinés à cette installation locale et ne sont jamais transmis aux conteneurs Vue ou Rust.

### Option : utilisateurs Active Directory via Samba

Une démonstration **compatible Active Directory** est disponible avec Samba AD DC
dans Docker (profil `ad`, sans ports publiés ni conteneur privilégié). Après le
démarrage normal, exécuter `python3 scripts/configure-ad.py` : le script crée
`user1` à `user4` avec le rôle `users` et `admin1` avec le rôle applicatif `admin`,
via des groupes AD et une fédération LDAPS en lecture seule. Les comptes locaux
restent conservés. Les mots de passe générés sont dans
`.secrets/ad-credentials.json`, ignoré par Git. `admin1` n’a aucun droit
d’administration Keycloak ou du domaine AD.

Voir [le guide Active Directory](docs/active-directory.md) pour le scénario,
les limites de la lecture seule, les redémarrages avec `--profile ad`, la
persistance et les vérifications `python3 scripts/verify-ad.py`.

## Scénarios de démonstration

### 1. Utilisateur standard

1. Ouvrir l’application et cliquer sur **Se connecter**.
2. Se connecter avec `demo-user`, puis changer le mot de passe initial si demandé.
3. La connexion depuis l’accueil mène directement au **Catalogue de services**. Le lien du catalogue est à gauche ; le bouton ☰ à droite ouvre **Paramètres** et **Déconnexion**. Ouvrir **Paramètres** et vérifier la page bleue, le badge `USER`, le nom Camille Utilisateur et les actions de sécurité du compte. Aucun lien vers la console d’administration n’est affiché pour ce compte.
4. Saisir `http://localhost:8080/admin` : **Accès refusé**. Le compte reste connecté.
5. Ouvrir le menu ☰ puis cliquer sur **Déconnexion** : retour à l’accueil public et fin de la session SSO courante.

### 2. Administrateur

1. Se connecter avec `demo-admin` et effectuer le changement de mot de passe initial.
2. Après l’arrivée sur le **Catalogue de services**, ouvrir le menu ☰ puis **Paramètres**. Vérifier la page mauve, le badge `ADMIN`, le nom Alex Administrateur, les actions de sécurité personnelles et le lien supplémentaire vers la console Keycloak.
3. Le catalogue et les paramètres sont accessibles même si le compte ne possède pas le rôle `users`.
4. Depuis **Paramètres**, ouvrir la console Keycloak. Le realm accessible est `olaninc`, pas `master`.

Le menu est identique sur ordinateur et mobile ; il se ferme après sélection, clic extérieur ou touche Échap. Les anciennes URLs `/user` et `/admin` mènent désormais à `/settings` (l’ancienne URL `/admin` reste réservée à `admin`).

### 3. Vérifier la protection effective de l’API

Sans jeton :

```sh
curl -i http://localhost:8080/api/user
curl -i http://localhost:8080/api/admin
```

Les deux réponses sont `401`, en JSON, avec `WWW-Authenticate: Bearer`, sans redirection HTML.

Les endpoints `/api/user` et `/api/admin` restent disponibles pour les exercices, mais ne sont plus appelés ni affichés dans l’interface. Avec `demo-user` connecté, ouvrir les outils de développement du navigateur → **Réseau**, puis accéder au catalogue et sélectionner `/api/catalog/services`. Le menu **Copier en tant que cURL** fournit une commande avec le jeton d’accès courant : remplacer le chemin par `/api/user` donne `200`, et par `/api/admin` donne `403`. Avec `demo-admin`, les deux donnent `200`.

**Ne pas enregistrer ni partager cette commande : elle contient un jeton utilisable.** Aucun jeton n’est affiché par l’application. Sa durée de vie est de cinq minutes.

### 4. Changer son mot de passe

Dans **☰ → Paramètres**, **Changer mon mot de passe** ouvre une action native Keycloak. Selon l’ancienneté de l’authentification, Keycloak peut demander de s’authentifier à nouveau. Choisir un nouveau mot de passe et vérifier le retour à la page Paramètres de la SPA. Se déconnecter et vérifier la connexion avec le nouveau mot de passe.

Le lien **Mon compte Keycloak** ouvre également la console de compte native complète. L’application ne lit ni ne stocke le mot de passe.

### 5. Configurer un OTP facultatif

1. Dans **☰ → Paramètres**, cliquer sur **Configurer mon OTP**.
2. Sur l’écran Keycloak, scanner le QR code avec une application d’authentification TOTP, saisir le code et un nom d’appareil si demandé.
3. Vérifier le retour à la SPA.
4. Attendre le code suivant dans l’application d’authentification, puis se déconnecter et se reconnecter : Keycloak demande désormais le code OTP de ce compte. Un code déjà utilisé peut être refusé pour éviter son rejeu.
5. Pour supprimer l’OTP, utiliser **Mon compte Keycloak → Sécurité du compte → Authentification à deux facteurs** (libellés variables selon la version).

L’OTP n’est pas imposé aux autres comptes. Une session SSO existante n’entraîne pas nécessairement une nouvelle demande d’OTP ; se déconnecter pour démontrer une authentification complète.

### 6. Réinitialiser par e-mail depuis l’administration

1. Dans la console Keycloak de `demo-admin`, ouvrir **Users / Utilisateurs → demo-user → Credentials / Identifiants**.
2. Utiliser **Credential reset / Réinitialisation des identifiants**, choisir l’action **Update Password**, puis envoyer l’e-mail. Selon la version, l’opération peut figurer dans le menu des actions du compte.
3. Ouvrir Mailpit, sélectionner le message adressé à `user@olaninc.test` et suivre le lien.
4. **Utiliser une fenêtre privée ou un profil navigateur distinct**, pour ne pas mélanger la session administrateur avec la session utilisateur.
5. Choisir le nouveau mot de passe sur l’écran natif Keycloak et vérifier la connexion utilisateur.

Si Keycloak ne propose pas un retour à l’application pour un e-mail envoyé depuis la console, rouvrir simplement l’URL de la SPA : ce n’est pas un formulaire applicatif.

### 7. Définir un mot de passe temporaire

Avec `demo-admin`, ouvrir **Users → demo-user → Credentials → Reset password**, saisir un mot de passe respectant la politique et conserver **Temporary** activé. Dans un autre profil navigateur, se connecter avec ce mot de passe : Keycloak impose son remplacement avant l’accès à la SPA.

### 8. Mot de passe oublié

Depuis la page de connexion native, cliquer sur **Mot de passe oublié**, saisir `demo-user` ou son adresse e-mail et consulter Mailpit. Suivre le lien dans le même profil navigateur que celui utilisé pour initier la demande, puis choisir le nouveau mot de passe. L’inscription libre reste désactivée.

Si ce compte possède déjà un OTP, le flux de récupération natif de Keycloak peut aussi demander de configurer un nouvel authentificateur. Suivre ces étapes sur les pages Keycloak ; pour démontrer uniquement le mot de passe, réaliser ce scénario avant l’activation OTP.

### 9. Expiration et persistance

- Le renouvellement des jetons d’accès se fait en mémoire via `keycloak-js`, avant les appels et à l’expiration du jeton.
- Pour simuler une session expirée : depuis un autre profil administrateur, révoquer les sessions de `demo-user`, puis attendre l’expiration du jeton ou relancer après cinq minutes. La SPA affiche **Session expirée** et attend un clic pour reconnecter.
- Une API validant localement un JWT peut encore l’accepter jusqu’à son expiration : la révocation SSO n’est **pas** une révocation instantanée de tous les jetons d’accès.
- Après `docker compose down` puis `docker compose up -d --wait`, vérifier que les nouveaux mots de passe et l’OTP sont conservés.

### 10. Catalogue commun et permissions

1. Avec l’un des deux comptes, ouvrir **Catalogue de services** dans la navigation. Le catalogue est initialement vide.
2. Avec `demo-admin`, cliquer sur **Ajouter un service**. Le formulaire apparaît dans la page : nom, catégorie, description et tarif mensuel en euros. L’identifiant est généré par Rust.
3. Enregistrer, modifier le tarif, puis supprimer le service avec la confirmation affichant son nom.
4. Dans un autre profil connecté avec `demo-user`, ouvrir la même page : les fiches sont visibles, sans formulaire ni boutons Ajouter/Modifier/Supprimer. Quitter puis rouvrir le catalogue pour voir les changements faits par l’autre compte.
5. Vérifier qu’un appel d’écriture avec un jeton utilisateur reçoit bien `403`, même sans passer par l’interface.
6. Redémarrer le backend : le catalogue redevient vide. Cela ne supprime ni les comptes Keycloak, ni les mots de passe, ni les OTP.

Les cinq champs sont l’identifiant, le nom, la catégorie, la description et le tarif mensuel. Catégories fixes : **Calcul, Stockage, Sauvegarde, Hébergement, Réseau**. Pas de règles métier supplémentaires : noms vides ou identiques acceptés ; seuls les types JSON, les catégories et la sécurité sont vérifiés. Le tarif est transporté en **centimes entiers non négatifs** (`monthly_price_cents`) et affiché en euros.

### Configurer le compte technique de chargement

Le client OIDC **confidentiel** `catalog-loader` utilise un compte de service et le flux OAuth 2.0 **Client Credentials**, sans compte utilisateur, navigateur, OTP ou mot de passe humain. Son seul rôle applicatif est `catalog-seed` et ses jetons ont l’audience `olaninc-api`. Il ne reçoit ni `admin`, ni `users`, ni permissions d’administration Keycloak.

Sur une installation vierge, le realm importé crée ce client et son compte de service ; Keycloak génère lui-même son secret. Pour mettre à niveau un realm déjà présent **sans réinitialiser les comptes**, puis récupérer le secret localement, utiliser depuis la racine du projet (Python 3 requis pour cet outil de configuration) :

```sh
python3 scripts/configure-catalog-loader.py
```

Ce script utilise le **CLI officiel Keycloak `kcadm.sh`** dans le conteneur. Il configure uniquement le rôle `catalog-seed` et le client `catalog-loader`, réutilise les identifiants techniques initiaux du conteneur et peut être rejoué sans renouveler le secret. Il ne réimporte pas le realm, ne supprime aucun volume et ne change aucun mot de passe humain. Si le compte initial a changé, fournir des identifiants d’administration valides du realm `master` via les variables `KEYCLOAK_ADMIN_USER` et `KEYCLOAK_ADMIN_PASSWORD` ; ne pas inscrire le mot de passe dans l’historique du shell.

Le secret courant est écrit dans **`.secrets/catalog-loader.secret`**, fichier local ignoré par Git, permissions `0600` dans un dossier `0700`. Il n’est pas affiché, copié dans une image Docker, monté dans le backend ou transmis à Vue. Ce fichier contient un secret en clair pour la démonstration locale : ne pas le partager. En dehors de cet exercice, le service appelant doit utiliser un gestionnaire de secrets et HTTPS.

### Charger les cinq services de démonstration par REST

**Aucun bouton de chargement dans la SPA et aucun Swagger UI.** `POST /api/catalog/seed` charge le JSON connu du projet, pas une URL externe ni un fichier envoyé par le client.

**Le jeton d’un administrateur humain ne suffit plus : il reçoit `403` sur cette route.** Le service demande son propre jeton d’accès à Keycloak, puis le transmet à Rust. Le secret du client ne s’envoie jamais directement à l’API Rust et aucun header `X-API-Key` n’est accepté à sa place.

Après la configuration ci-dessus, exécuter depuis la racine du projet :

```sh
API_BASE=http://localhost:8080
TOKEN=$(
  curl --fail --silent --show-error \
    --data 'grant_type=client_credentials' \
    --data 'client_id=catalog-loader' \
    --data-urlencode 'client_secret@.secrets/catalog-loader.secret' \
    http://localhost:8081/realms/olaninc/protocol/openid-connect/token \
  | python3 -c 'import json, sys; print(json.load(sys.stdin)["access_token"])'
)

curl -i -X POST "$API_BASE/api/catalog/seed" \
  -H "Authorization: Bearer $TOKEN"

unset TOKEN
```

La syntaxe `client_secret@...` fait lire le fichier par `curl`, sans mettre sa valeur dans la commande. Adapter `API_BASE` si le frontend utilise un autre port. Le jeton reste temporairement en mémoire dans la variable shell, sans affichage ni écriture sur disque. Pour consulter les services, utiliser ensuite la SPA avec `demo-user` ou `demo-admin` : le compte technique n’a **pas** le droit de lecture.

Avec Postman ou un outil similaire, obtenir d’abord un jeton OAuth 2.0 avec **Client Credentials**, le token endpoint ci-dessus, le Client ID `catalog-loader` et son secret local (authentification du client par HTTP Basic ou dans le corps). Envoyer ensuite un **POST** à `/api/catalog/seed`, authentification **Bearer Token**, corps vide. Ne pas enregistrer ni partager le secret ou les jetons dans les collections exportées.

Le jeton expire après cinq minutes dans cette démo. Aucun refresh token n’est émis par défaut : redemander un jeton avec les identifiants du client à expiration. Pour renouveler le secret, utiliser **Clients → catalog-loader → Credentials** dans la console native, puis rejouer le script pour synchroniser le fichier local. Cela ne révoque pas instantanément les JWT déjà émis auprès d’une API qui les valide localement ; leur expiration reste applicable.

**Attention : chaque appel remplace intégralement le catalogue courant**, y compris les ajouts et modifications, et génère de nouveaux identifiants. Réponse : `200` avec `{"count":5}`. Les services préchargés sont Machine virtuelle, Serveur dédié, Stockage objet (forfait 1 To), Sauvegarde managée et Hébergement en baie. Le fichier source est `backend/data/demo_catalog.json`, embarqué dans le binaire : le modifier nécessite de reconstruire le backend, puis de rappeler la route.

| Endpoint | Rôles | Réponse de succès |
| --- | --- | --- |
| `GET /api/catalog/services` | `users`, `admin` | `200`, tableau de services |
| `GET /api/catalog/services/{id}` | `users`, `admin` | `200`, service |
| `POST /api/catalog/services` | `admin` | `201`, service créé |
| `PUT /api/catalog/services/{id}` | `admin` | `200`, service remplacé |
| `DELETE /api/catalog/services/{id}` | `admin` | `204`, aucun corps |
| `POST /api/catalog/seed` | `catalog-seed` uniquement | `200`, nombre de services chargés |

Pour créer ou modifier, envoyer `Content-Type: application/json` avec les quatre champs modifiables (l’identifiant ne se saisit pas) :

```json
{
  "name": "Machine virtuelle",
  "category": "compute",
  "description": "VM avec 2 vCPU, 4 Go de RAM et 80 Go de SSD.",
  "monthly_price_cents": 2500
}
```

Codes de catégories : `compute`, `storage`, `backup`, `hosting`, `network`. Un service inexistant renvoie `404`, un corps incompatible ou une catégorie inconnue `400`. Les protections `401`/`403` restent identiques aux endpoints de démonstration.

## Tests et vérifications

Sur la machine, avec Rust récent et Node ≥ 20.19 :

```sh
cargo test --locked --manifest-path backend/Cargo.toml
cargo clippy --locked --manifest-path backend/Cargo.toml --all-targets -- -D warnings
```

Dans `frontend/` :

```sh
npm ci
npm run build
npm audit
```

Les tests Rust lancent un serveur JWKS local avec une clé RSA de test et exercent le vrai routeur, le middleware et la validation cryptographique. Ils couvrent notamment : signatures falsifiées, algorithme non autorisé, expiration, issuer, audience simple ou multiple, champs obligatoires, ID/refresh tokens refusés, rôles, `401`/`403`, cache JWKS et défaillance des clés. Ils ne nécessitent pas de Keycloak ni de Docker.

Les tests du catalogue couvrent également le CRUD, les identifiants concurrents, le stockage partagé, le démarrage vide, le remplacement par le JSON prédéfini et les permissions de tous les nouveaux endpoints.

Ils vérifient que le seed refuse `admin` seul (`403`), accepte `catalog-seed`, et que ce rôle technique ne permet ni lecture ni CRUD, ni accès à `/api/user` ou `/api/admin`. Une clé brute passée dans `X-API-Key` sans JWT reste refusée (`401`).

Les parcours navigateur sont vérifiés à l’aide des scénarios ci-dessus ; aucune suite E2E permanente n’est ajoutée en V1.

### Développer sans reconstruire les images

Avec les ports par défaut disponibles, démarrer uniquement l’IAM :

```sh
docker compose up -d --wait keycloak mailpit
```

Dans `backend/`, lancer le resource server avec les clés accessibles depuis la machine :

```sh
OIDC_ISSUER=http://localhost:8081/realms/olaninc \
OIDC_AUDIENCE=olaninc-api \
OIDC_JWKS_URL=http://localhost:8081/realms/olaninc/protocol/openid-connect/certs \
cargo run --locked
```

Dans `frontend/`, lancer `npm ci`, puis `npm run dev`. Vite écoute sur `http://localhost:8080` et relaie `/api` vers `127.0.0.1:3000`. Ne pas lancer simultanément les services frontend/backend Docker sur ces mêmes ports. Pour une démonstration normale, préférer le démarrage entièrement Docker ci-dessus.

## Arrêter et remettre à zéro

Le catalogue n’a pas de volume : **tout arrêt/redémarrage du processus Rust le remet à vide**, même si les données IAM sont conservées.

```sh
# Arrêter en conservant comptes, mots de passe, OTP et e-mails
docker compose down

# Redémarrer sans reconstruire
docker compose up -d --wait
```

**Attention : les commandes suivantes suppriment les données Keycloak et Mailpit de ce projet.** Les mots de passe initiaux temporaires seront rétablis au prochain import.

```sh
docker compose down --volumes
docker compose up --build -d --wait
```

L’import de `keycloak/olaninc-realm.json` s’exécute **uniquement si le realm n’existe pas**. Modifier ce fichier ne reconfigure pas un realm existant : appliquer la modification dans la console ou effectuer une remise à zéro volontaire.

Après une remise à zéro des volumes, Keycloak génère un nouveau secret pour `catalog-loader` : rejouer `python3 scripts/configure-catalog-loader.py` avant les appels de chargement pour synchroniser le fichier local. Pour ajouter seulement ce client à une installation existante, ce script suffit : aucune remise à zéro n’est nécessaire.

## Ports déjà occupés

Sans arrêter un autre projet, choisir des ports libres **avant le premier import** :

```sh
FRONTEND_HTTP_PORT=8082 MAILPIT_HTTP_PORT=8026 docker compose up --build -d --wait
```

La SPA est alors sur http://localhost:8082 et Mailpit sur http://localhost:8026. Keycloak reste sur `8081`. Réutiliser les mêmes variables pour les prochains démarrages, ou les conserver dans un fichier `.env` local ignoré :

```dotenv
FRONTEND_HTTP_PORT=8082
MAILPIT_HTTP_PORT=8026
```

Le port frontend fait partie des URLs OIDC importées. Pour le changer après initialisation, modifier les URLs et origines du client `olaninc-spa` dans Keycloak, ou réinitialiser volontairement les données. Le port Keycloak est fixe pour conserver un issuer unique.

## Dépannage et structure

```sh
docker compose ps
docker compose logs --tail=100 keycloak backend frontend mailpit
```

- `401` : jeton absent, expiré, invalide ou destiné à une autre API.
- `403` : jeton valide, rôle applicatif absent. Les permissions `realm-management` ne remplacent pas le rôle applicatif `admin`.
- `503` de l’API : récupération des clés impossible ; aucun accès n’est autorisé par défaut.
- `502` du proxy : vérifier le service backend.
- `invalid_redirect_uri` : vérifier `localhost`, le port et les URLs du client importé.
- Rechargement de page : les jetons en mémoire sont perdus. Un contrôle SSO silencieux peut les récupérer ; si le navigateur le bloque, cliquer sur **Se connecter** retrouve la session SSO sans nécessairement demander le mot de passe.

```text
compose.yaml                  Services et réseau local
keycloak/olaninc-realm.json    Configuration initiale reproductible
frontend/                     Vue, keycloak-js, proxy Nginx
backend/src/auth/mod.rs       Validation JWT, middleware et erreurs de sécurité
backend/src/auth/jwks.rs      Récupération et cache des clés publiques
backend/src/lib.rs            Routes et politiques de rôles
backend/src/catalog/          Cœur catalogue, port et adaptateurs REST/mémoire
backend/data/demo_catalog.json Catalogue fictif chargé uniquement par POST /seed
scripts/configure-catalog-loader.py Configuration native du client technique et récupération locale du secret
backend/tests/security.rs     Tests de sécurité sans IAM externe
backend/tests/catalog_api.rs  Tests API et permissions du catalogue
backend/tests/catalog_service.rs Tests applicatifs et stockage en mémoire
docs/architecture.md          Explications pédagogiques
```

Voir [le document pédagogique](docs/architecture.md) pour comprendre PKCE, les audiences, les rôles et les limites de cette démonstration.
