# Comprendre la démonstration IAM

## 1. Frontières de responsabilité

```text
Navigateur
  ├── http://localhost:8080 → Nginx → fichiers Vue
  │                           └── /api/* → Axum:3000
  └── http://localhost:8081 → Keycloak:8080
                                  └── SMTP → Mailpit:1025
                                             └── UI : localhost:8025

Axum ── téléchargement des clés publiques ──→ Keycloak /certs
```

Les ports navigateur peuvent être adaptés pour Vue et Mailpit ; ces exemples utilisent les valeurs par défaut. Tous les ports publiés sont liés à `127.0.0.1`. Le backend et le SMTP ne sont pas publiés.

| Composant | Responsable de | Ne fait jamais |
| --- | --- | --- |
| Vue | Connexion déclenchée par l’utilisateur, callback OIDC, navigation, appels API | Formulaire de mot de passe, génération OTP, administration IAM |
| Axum | Validation du jeton d’accès, contrôle des rôles et exposition REST du catalogue | Redirection vers la connexion, échange de code, gestion des utilisateurs |
| Keycloak | Identités, sessions, authentification, mots de passe, OTP, rôles, consoles, e-mails | Logique métier de l’API |
| Mailpit | Capture et consultation locale des messages SMTP | Distribution de vrais e-mails |

Rust n’utilise pas de base de données : le catalogue est uniquement en mémoire et disparaît au redémarrage du backend. Les données IAM de Keycloak et les messages Mailpit sont persistés dans deux volumes Docker distincts.

## 2. Authorization Code Flow avec PKCE

1. La SPA appelle `keycloak.login()` après un clic.
2. `keycloak-js` crée un état OIDC et un vérificateur PKCE aléatoire, puis redirige le navigateur vers Keycloak avec son challenge SHA-256 (**S256**).
3. Keycloak présente ses formulaires natifs. Un mot de passe temporaire provoque l’action requise native `UPDATE_PASSWORD`. Si un OTP est configuré sur le compte, le flux navigateur natif le demande.
4. Keycloak renvoie un **code d’autorisation** vers l’URL exacte autorisée de la SPA (`http://localhost:8080/`).
5. Dans le navigateur, la bibliothèque échange le code et le vérificateur PKCE contre les jetons. **Rust ne participe pas à cet échange.**
6. La SPA utilise le jeton d’accès dans `Authorization: Bearer …` pour appeler l’API.

PKCE lie l’échange du code à l’instance ayant lancé la demande. Ce n’est pas un secret de client. Une SPA est un **client public** et ne peut conserver de secret ; son client Keycloak a donc `publicClient: true`. Le flux implicite, le password grant et les comptes de service sont désactivés. Le serveur exige S256, en plus de la configuration de l’adaptateur.

Les retours de connexion et de déconnexion sont exacts, sans wildcard. Le client autorise aussi explicitement `/silent-check-sso.html` pour le contrôle SSO silencieux. Le routeur Vue est initialisé **après** la bibliothèque Keycloak, afin de ne pas altérer le callback avant son traitement. Un délai maximal d’initialisation permet d’afficher une erreur plutôt que de laisser une page blanche si un iframe ne répond pas.

## 3. Les trois jetons ne sont pas interchangeables

| Jeton | Usage | Transmis à Rust ? |
| --- | --- | --- |
| Access token | Autoriser un appel à l’audience `olaninc-api` | Oui |
| ID token | Décrire l’authentification de l’utilisateur auprès de la SPA ; utilisé aussi pour la déconnexion native | Non |
| Refresh token | Obtenir un nouveau jeton d’accès tant que la session l’autorise | Non |

L’adaptateur conserve les jetons uniquement en mémoire. L’application ne les copie ni dans `localStorage`, ni dans `sessionStorage`, ni dans un cookie. `sessionStorage` contient au plus un chemin de navigation de retour (`/settings` ou `/catalog`, avec prise en charge des anciennes URLs `/user` et `/admin`) ; il ne constitue pas une preuve d’authentification.

L’état OIDC transitoire et les cookies de session sont gérés par les bibliothèques et Keycloak. « Jetons en mémoire » ne signifie donc pas « aucune donnée temporaire du protocole dans le navigateur ».

Le renouvellement est tenté avant chaque appel API, avec une marge de 30 secondes, et lors de l’expiration du jeton. En cas d’échec, la SPA efface son état, revient à l’accueil et affiche **Session expirée**. Elle ne redirige pas automatiquement vers une nouvelle connexion.

Un rechargement perd les jetons en mémoire. Le `check-sso` silencieux tente de retrouver la session native ; le fallback par redirection automatique est désactivé pour les navigateurs qui bloquent cet iframe. Le bouton de connexion permet alors de retrouver la session SSO. La surveillance de session par iframe est désactivée dans cette démo HTTP locale : une déconnexion externe sera détectée lors du renouvellement, pas instantanément.

## 4. Clients, audience et rôles

### Client `olaninc-spa`

- Client public OIDC, Authorization Code Flow, PKCE S256.
- Origine autorisée : celle de la SPA ; seuls les échanges navigateur → Keycloak nécessitent ce CORS.
- Scopes par défaut `basic` (notamment `sub`), `profile`, `email` et `web-origins`.
- `fullScopeAllowed: false` et scope mapping explicite des rôles `users` et `admin`.
- Mapper d’audience explicite vers `olaninc-api`, uniquement dans l’access token.
- Mapper de rôles de realm vers `realm_access.roles`, uniquement dans l’access token.

### Client `olaninc-api`

Il matérialise l’audience de l’API. Il est **bearer-only**, sans flux de connexion ni compte de service. Aucun secret de ce client n’est nécessaire au resource server : la vérification repose sur les clés **publiques** de Keycloak.

### Client technique `catalog-loader`

Ce client est distinct de la SPA et de l’API : authentification client par secret, compte de service activé, flux navigateur/implicite/password grant désactivés. Il obtient un JWT d’accès en son propre nom avec `grant_type=client_credentials`. Le seul rôle applicatif autorisé par ses scope mappings et attribué à son compte de service est `catalog-seed` ; un mapper ajoute l’audience `olaninc-api` et un autre les rôles de realm. Le scope `basic` conserve notamment le claim `sub` exigé par Rust. Les jetons ne contiennent ni rôle applicatif `admin`, ni permissions `realm-management`.

```text
Service appelant ── client_id + client_secret ──→ Keycloak /token
Service appelant ←──── JWT d’accès court ─────── Keycloak
Service appelant ── Authorization: Bearer JWT ──→ Rust /api/catalog/seed
```

Le secret authentifie le client auprès de **Keycloak**, pas auprès de Rust. Ce n’est pas une clé statique `X-API-Key`. Les contrôles de signature, expiration, issuer, audience et type de jeton existants s’appliquent aussi aux jetons techniques. Aucun refresh token n’est émis par défaut ; le client renouvelle son accès par une nouvelle demande Client Credentials.

Le realm JSON initialise ce client sans secret prédéfini : Keycloak le génère. Pour une installation existante, `scripts/configure-catalog-loader.py` applique uniquement cette configuration avec le CLI officiel `kcadm.sh`, sans remplacer le realm ni les comptes humains. La configuration client et ses mappers sont lus depuis le même JSON que l’import initial. L’authentification d’administration est réservée à cette maintenance, jamais au backend applicatif. Le secret est récupéré dans `.secrets/catalog-loader.secret`, fichier local ignoré et limité au propriétaire ; aucune valeur n’est fournie à Vue ou Rust.

Références officielles Keycloak 26.8.1 : [Using a service account](https://www.keycloak.org/docs/26.8.1/server_admin/#_service_accounts), [Client credentials](https://www.keycloak.org/securing-apps/oidc-layers#_client_credentials), [Audience support](https://www.keycloak.org/docs/26.8.1/server_admin/#audience-support). Les rôles du jeton sont l’intersection des rôles du compte de service et des rôles autorisés par les scope mappings. Le rôle `catalog-seed` est notre permission applicative, pas une fonction intégrée de Keycloak ni une preuve intrinsèque qu’un sujet est une machine : nous le réservons au compte technique dans la configuration.

### Deux systèmes de permissions distincts

| Permission | Nature | Effet |
| --- | --- | --- |
| `users` | Rôle de realm applicatif | `/api/user` et consultation du catalogue |
| `admin` | Rôle de realm applicatif | `/api/user`, `/api/admin`, consultation et CRUD du catalogue, sans seed |
| `catalog-seed` | Rôle de realm applicatif réservé au compte technique | Uniquement `POST /api/catalog/seed` |
| `realm-management:realm-admin` | Rôle client d’administration Keycloak | Administration complète de `olaninc` |

`demo-admin` reçoit explicitement `admin` et `realm-management:realm-admin`, pas `catalog-seed`. Le rôle applicatif `admin` **n’est pas composite** avec `realm-admin` et ne l’accorde pas automatiquement. Les permissions d’administration ne sont pas nécessaires dans le jeton de la SPA ; la console d’administration utilise son propre client et sa session SSO.

Le compte technique `bootstrap-admin` est créé dans `master`, séparément de l’import du realm. Le compte `demo-admin` ne peut pas administrer `master`, créer d’autres realms ou administrer l’instance entière. Il peut néanmoins modifier toute la configuration de `olaninc`, y compris casser les clients ou modifier les permissions : c’est le niveau de droits choisi pour cet exercice.

L’utilisateur administrateur ne reçoit pas forcément `users` : le backend autorise explicitement **users OU admin** sur l’endpoint utilisateur.

## 5. Validation dans Rust

`headers` lit le schéma Bearer, `jsonwebtoken` effectue la validation cryptographique et des claims, et un middleware Axum commun applique l’authentification avant les handlers. Les handlers ne font qu’imposer la politique de rôles.

Le module `backend/src/auth/jwks.rs` gère uniquement le téléchargement, le cache et la recherche des clés publiques par `kid`. Il ne dépend pas d’Axum et ne décide pas des réponses HTTP. `backend/src/auth/mod.rs` conserve les règles de validation JWT, la vérification de l’usage des clés, le middleware et la traduction des erreurs JWKS en erreurs API. L’interface publique `auth::{AuthConfig, Authenticator}` reste inchangée.

Pour un appel protégé :

1. Exiger un unique header `Authorization` correctement formé.
2. Lire uniquement l’en-tête JWT non fiable pour identifier `kid` et refuser les algorithmes autres que **RS256**.
3. Chercher la clé dans le JWKS configuré. L’URL n’est **jamais** fournie par le jeton ; les redirections HTTP sont refusées.
4. Vérifier la signature RSA, `exp`, `iss`, `aud` et, s’il existe, `nbf`. Les claims `exp`, `iss`, `aud` et `sub` sont obligatoires. La marge d’expiration est explicitement nulle pour l’exercice.
5. Exiger un `sub` non vide et le claim Keycloak `typ: Bearer`, afin de ne pas accepter un ID token ou un refresh token comme jeton d’accès.
6. Lire les rôles seulement après validation et appliquer les règles du endpoint.

Une audience peut être une chaîne ou une liste contenant `olaninc-api`. Le simple fait que `azp` vaille `olaninc-spa` ne remplace jamais une audience correcte.

| Résultat | Réponse |
| --- | --- |
| Jeton absent, mal formé, expiré ou invalide | `401`, JSON et `WWW-Authenticate: Bearer` |
| Jeton valide, rôle requis absent | `403`, JSON |
| Clés nécessaires indisponibles | `503`, JSON, aucun accès accordé |
| Jeton et rôle corrects | `200`, message JSON |

Aucune erreur API ne redirige vers une page de connexion. Le `503` distingue une panne de l’infrastructure de confiance d’un mauvais jeton.

### Cache et rotation de clés

Les clés sont chargées au démarrage, puis mises en cache cinq minutes. Un `kid` inconnu peut déclencher un rafraîchissement anticipé ; les téléchargements sont espacés d’au moins 30 secondes, y compris après un échec. Un verrou empêche les téléchargements concurrents en cascade.

Pendant ce délai, un nouveau `kid` légitime peut être refusé temporairement (`401`). Une fois le cache expiré, une récupération impossible provoque un refus fermé (`503`), sans prolonger indéfiniment la confiance dans d’anciennes clés. C’est une politique simple pour l’exercice, pas un système de haute disponibilité.

## 6. Issuer public et réseau Docker

Keycloak annonce toujours :

```text
http://localhost:8081/realms/olaninc
```

Ce nom est valide pour le navigateur. Dans le conteneur Rust, `localhost` désignerait Rust lui-même. Les deux configurations sont donc séparées :

```text
OIDC_ISSUER   = http://localhost:8081/realms/olaninc
OIDC_AUDIENCE = olaninc-api
OIDC_JWKS_URL = http://keycloak:8080/realms/olaninc/protocol/openid-connect/certs
```

L’adresse de transport des clés est interne, mais la comparaison de l’issuer reste stricte et publique. On ne désactive pas la validation de l’issuer pour contourner un problème réseau.

Le navigateur appelle l’API sur la même origine que la SPA. Nginx relaie `/api/*` sans changer le chemin ni convertir les erreurs en HTML. Le backend n’a donc pas besoin d’ouvrir un CORS permissif.

## 7. Opérations natives et e-mails

Les boutons Vue utilisent les **Application Initiated Actions** : `keycloak.login({ action: 'UPDATE_PASSWORD' })` ou `CONFIGURE_TOTP`. La bibliothèque construit le parcours OIDC, Keycloak exécute l’action et la SPA traite le retour standard. Annuler une action n’est pas une réussite, mais n’exige pas d’écran IAM applicatif supplémentaire : l’utilisateur retrouve simplement son espace.

La navigation sécurisée affiche le catalogue à gauche et un menu de compte ☰ à droite, avec **Paramètres** et **Déconnexion**, sur ordinateur comme sur mobile. `/settings` regroupe les liens vers les fonctions natives de sécurité pour les deux rôles ; le lien vers la console d’administration n’apparaît que pour `admin`. La page et la navigation sont bleues pour `users`, mauves pour `admin`. Le retour des actions natives conserve la destination `/settings`. Les anciennes pages `/user` et `/admin` ne sont plus des écrans distincts : elles redirigent vers les paramètres, en conservant le contrôle `admin` sur l’ancienne URL `/admin`.

La console de compte, la console d’administration, les formulaires, le flux OTP conditionnel natif et les e-mails utilisent tous les thèmes et comportements natifs de Keycloak. **Le theming Keycloak est hors périmètre.** Les couleurs bleue/violette ne concernent que Vue.

Les actions requises et flux d’authentification natifs sont ceux créés par Keycloak à l’import. Seul `UPDATE_PASSWORD` est imposé initialement aux deux comptes ; `CONFIGURE_TOTP` n’est imposé à personne.

Keycloak envoie son SMTP à `mailpit:1025`, sans authentification ni TLS dans le réseau Docker local. Aucune adresse SMTP externe n’est configurée et aucune option de relais n’est activée dans Mailpit. Les adresses fictives sont marquées vérifiées à l’initialisation pour éviter d’ajouter un parcours de vérification hors scope.

## 8. Reproductibilité et limites assumées

- Les versions des outils frontend sont fixées et `package-lock.json` est fourni ; Rust utilise `Cargo.lock` et les builds utilisent les modes verrouillés.
- Les images Keycloak et Mailpit ont une version explicite. Les images de compilation Node/Rust et de runtime Nginx/Debian restent des tags ; ce n’est pas une garantie de reproductibilité bit à bit.
- L’import au démarrage **ne remplace pas** un realm existant. La persistance permet de conserver les exercices réalisés.
- La base embarquée `dev-file` convient à une instance de démonstration, pas à un déploiement de production ou multi-instance.
- Les identifiants humains initiaux sont publics et réservés à l’environnement local. Les fichiers d’initialisation et Docker connaissent ces valeurs ; l’application Vue/Rust ne les reçoit pas. Le secret technique `catalog-loader`, généré séparément par Keycloak, n’est pas inscrit dans ces fichiers et reste dans un fichier local protégé pour l’outil appelant.
- La validation locale d’un JWT ne vérifie pas à chaque requête la session Keycloak : la révocation et les changements de rôles sont observés au renouvellement ou à l’expiration du jeton, pas immédiatement.
- Les tests backend utilisent de vraies signatures et un faux serveur de clés ; ils ne prouvent pas à eux seuls que l’import Keycloak est correct. Le guide de scénarios couvre cette intégration.
- Pas d’inscription libre, de politique OTP obligatoire, de formulaires IAM personnalisés, de thème personnalisé, de BFF, de base applicative, d’introspection distante ou de déploiement public.

Évolutions possibles après V1 : tests navigateur pérennes, inscription native Keycloak avec rôle par défaut maîtrisé, thème sur demande explicite et durcissement pour un véritable environnement cible. Aucune de ces évolutions n’est activée implicitement.

## 9. Catalogue : une architecture hexagonale légère

Le catalogue constitue une ressource métier fictive, volontairement distincte de l’IAM. Ajouter un service ou modifier un prix ne crée aucune infrastructure et ne modifie aucun utilisateur ni rôle Keycloak.

```text
Vue / curl / Postman
       │ HTTP + Bearer
       ▼
auth::authenticate                  Validation JWT existante
       ▼
catalog_http.rs                     Adaptateur entrant : JSON, rôles, HTTP
       ▼
catalog_service.rs                  Cas d’usage, types du domaine
       │
       ▼
CatalogRepository                  Port sortant défini côté cœur
       ▲
       │ implémenté par
in_memory_repository.rs             Adaptateur sortant : collection et verrou
```

### Rôles des fichiers

| Fichier dans `backend/src/catalog/` | Responsabilité |
| --- | --- |
| `domain.rs` | Types `DatacenterService`, `ServiceDraft` et cinq catégories ; aucune dépendance à Axum, Serde, JWT ou Keycloak |
| `catalog_service.rs` | Lister, consulter, créer, modifier, supprimer et remplacer ; dépend uniquement du domaine et du port |
| `catalog_repository.rs` | Trait `CatalogRepository`, contrat de stockage synchrone `Send + Sync` |
| `in_memory_repository.rs` | Implémentation avec `BTreeMap`, compteur d’identifiants et `Mutex` ; liste ordonnée par identifiant |
| `catalog_http.rs` | Routes Axum, DTO JSON, contrôles de rôles et conversion des erreurs métier en HTTP |
| `demo_catalog.rs` | Adaptateur de lecture du JSON prédéfini embarqué via `include_str!` |

`backend/src/catalog.rs` déclare les modules. `main.rs` construit une instance du repository en mémoire, l’injecte dans `CatalogService` et appelle `app_with_catalog`. `app(auth)` reste disponible comme constructeur pratique avec un stockage neuf, notamment pour les tests existants.

Le repository **implémente** le port dont dépend le service applicatif. Remplacer la mémoire par un autre stockage ne doit pas imposer de changement au domaine ni aux handlers. Il n’y a pas de trait par cas d’usage : les méthodes du service constituent son interface d’entrée.

### Sécurité à la frontière HTTP

Les routes du catalogue passent toutes par le middleware JWT existant. Après authentification, l’adaptateur HTTP applique les rôles : `users` ou `admin` pour les lectures, `admin` pour le CRUD, et exclusivement `catalog-seed` pour le chargement intégral. Le domaine et le service applicatif ne connaissent pas les claims Keycloak. Ils ne doivent donc pas être exposés par un nouvel adaptateur sans y appliquer la politique d’autorisation correspondante.

Les boutons Vue sont uniquement une aide visuelle, pas la protection réelle. Les erreurs métier (`ServiceNotFound`) n’ont pas de code HTTP dans le cœur ; l’adaptateur les convertit en `404`. Les corps JSON incompatibles et catégories inconnues donnent `400`, sans ajouter de règles métier de longueur ou d’unicité.

### État, concurrence et chargement

Le repository est partagé par toutes les requêtes du processus. Ses opérations synchrones utilisent de courtes sections critiques sans `.await`. Le remplacement intégral prend un seul verrou : une lecture ne peut pas observer un catalogue à moitié chargé. Les identifiants restent croissants pendant la vie du processus, y compris lors d’un rechargement, pour éviter de réaffecter à un nouveau service l’identifiant encore détenu par un ancien écran.

Le backend démarre vide. Seul `POST /api/catalog/seed`, avec un jeton portant `catalog-seed` obtenu par le client technique, charge les cinq services du fichier `backend/data/demo_catalog.json` et **remplace toutes les données courantes**. Le jeton d’un administrateur humain ne suffit plus. Le fichier est désérialisé avant le remplacement : une erreur de lecture ne détruit pas le catalogue courant. Il n’existe ni bouton de chargement, ni endpoint d’upload, ni URL externe configurable, ni nouvel accès à l’API d’administration Keycloak dans Rust.

Le prix est un entier de centimes dans le domaine et l’API. Vue le convertit pour le formulaire et l’affiche en euros. La page `/catalog` est commune : consultation pour tous les comptes applicatifs, formulaire d’ajout/modification dans la page et suppression avec confirmation pour les administrateurs. Le catalogue se charge à l’ouverture et après chaque modification locale. Les autres sessions voient les changements en quittant puis en rouvrant le catalogue ; aucun bouton d’actualisation ni mécanisme temps réel n’est ajouté.

Les tests applicatifs s’exécutent sans HTTP ni JWT ; les tests de l’adaptateur REST utilisent le même serveur JWKS local que les tests de sécurité. Un nouveau repository représente un redémarrage et doit toujours commencer vide. Le stockage en mémoire ne fournit ni durabilité, ni synchronisation entre plusieurs processus : c’est un choix explicite de démonstration.
