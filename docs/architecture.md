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
| Cost Monitor | Délégation catalogue en lecture seule, seuils, contrôle chaque minute et e-mails métier | Modification du catalogue ou administration IAM |

Rust n’utilise pas de base de données : le catalogue est uniquement en mémoire et disparaît au redémarrage du backend. Les données IAM de Keycloak et les messages Mailpit sont persistés dans deux volumes Docker distincts.

### Fédération Active Directory optionnelle

Le profil Compose `ad` ajoute Samba AD DC, construit à partir de Debian et de
ses paquets Samba (pas une image tierce opaque). Son volume `samba-data` conserve
le domaine `ad.olaninc.test`, les utilisateurs, groupes, mots de passe et clés
TLS. Le conteneur n’est pas privilégié et ne publie aucun port sur le Mac.

```text
Vue ── OIDC/PKCE ──→ Keycloak ── LDAPS + compte de lecture ──→ Samba AD
                         └── groupes AD → groupes Keycloak → rôles JWT
Rust ←── Bearer JWT émis par Keycloak, quelle que soit l’origine du compte
```

`infrastructure/active-directory/scripts/configure-ad.py` initialise les secrets locaux et le domaine, monte
l’autorité publique Samba dans le truststore Keycloak, puis utilise le CLI
officiel pour configurer le fournisseur LDAP en lecture seule. Les mots de passe
sont vérifiés dans l’annuaire ; seuls les profils sont importés. `objectGUID`
relie chaque profil à son entrée AD. Aucun fournisseur n’est ajouté au JSON du
realm avec un secret prédéfini : le script est rejouable sur une installation
vierge ou existante, sans remplacement des comptes locaux.

Les quatre comptes `user1` à `user4` appartiennent à `catalog-users` et obtiennent
`users`. `admin1` appartient à `catalog-admins` et obtient uniquement le rôle
applicatif `admin`, pas `realm-management:realm-admin` ni un privilège de domaine.
Les groupes sont importés sous `/AD` et leur membership est lu depuis LDAP,
sans cache utilisateur pour cette démo. Un changement de groupe se reflète dans
les nouveaux jetons, pas dans les JWT déjà émis. L’API continue à valider les
mêmes rôles et la même audience : aucun code LDAP dans Vue ou Rust.

Ce scénario démontre la fédération d’un annuaire **compatible AD**, pas une
installation Microsoft, une jonction Windows, le SSO Kerberos ou des GPO.
L’annuaire en lecture seule ne permet pas les modifications de mots de passe
ou d’attributs LDAP depuis les écrans IAM. Les comptes locaux restent utilisables
pour ces parcours. Voir [le guide de démonstration](active-directory.md).

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
- Scopes métier `catalog:read`, `catalog:write`, `selections:read` et `selections:write` ; ils ne remplacent pas les contrôles de rôle et de propriété.
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

Le realm JSON initialise ce client sans secret prédéfini : Keycloak le génère. Pour une installation existante, `infrastructure/keycloak/scripts/configure-catalog-loader.py` applique uniquement cette configuration avec le CLI officiel `kcadm.sh`, sans remplacer le realm ni les comptes humains. La configuration client et ses mappers sont lus depuis le même JSON que l’import initial. L’authentification d’administration est réservée à cette maintenance, jamais au backend applicatif. Le secret est récupéré dans `.secrets/catalog-loader.secret`, fichier local ignoré et limité au propriétaire ; aucune valeur n’est fournie à Vue ou Rust.

Références officielles Keycloak 26.8.1 : [Using a service account](https://www.keycloak.org/docs/26.8.1/server_admin/#_service_accounts), [Client credentials](https://www.keycloak.org/securing-apps/oidc-layers#_client_credentials), [Audience support](https://www.keycloak.org/docs/26.8.1/server_admin/#audience-support). Les rôles du jeton sont l’intersection des rôles du compte de service et des rôles autorisés par les scope mappings. Le rôle `catalog-seed` est notre permission applicative, pas une fonction intégrée de Keycloak ni une preuve intrinsèque qu’un sujet est une machine : nous le réservons au compte technique dans la configuration.

### Deux systèmes de permissions distincts

| Permission | Nature | Effet |
| --- | --- | --- |
| `users` | Rôle de realm applicatif | Consultation du catalogue et gestion de ses sélections |
| `admin` | Rôle de realm applicatif | Consultation et CRUD du catalogue, gestion de ses sélections, sans seed |
| `catalog-seed` | Rôle de realm applicatif réservé au compte technique | Uniquement `POST /api/catalog/seed` |
| `realm-management:realm-admin` | Rôle client d’administration Keycloak | Administration complète de `olaninc` |

`demo-admin` reçoit explicitement `admin` et `realm-management:realm-admin`, pas `catalog-seed`. Le rôle applicatif `admin` **n’est pas composite** avec `realm-admin` et ne l’accorde pas automatiquement. Les permissions d’administration ne sont pas nécessaires dans le jeton de la SPA ; la console d’administration utilise son propre client et sa session SSO.

Le compte technique `bootstrap-admin` est créé dans `master`, séparément de l’import du realm. Le compte `demo-admin` ne peut pas administrer `master`, créer d’autres realms ou administrer l’instance entière. Il peut néanmoins modifier toute la configuration de `olaninc`, y compris casser les clients ou modifier les permissions : c’est le niveau de droits choisi pour cet exercice.

L’utilisateur administrateur ne reçoit pas forcément `users` : le backend autorise explicitement **users OU admin** pour consulter le catalogue et gérer ses sélections.

## 5. Validation dans Rust

`headers` lit le schéma Bearer, `jsonwebtoken` effectue la validation cryptographique et des claims, et un middleware Axum commun applique l’authentification avant les handlers. Les handlers ne font qu’imposer la politique de rôles.

Le module `catalog-services/backend/src/infrastructure/auth/jwks.rs` gère uniquement le téléchargement, le cache et la recherche des clés publiques par `kid`. Il ne dépend pas d’Axum et ne décide pas des réponses HTTP. `catalog-services/backend/src/infrastructure/auth/jwt.rs` conserve les règles de validation JWT, la vérification de l’usage des clés, le middleware et la traduction des erreurs JWKS en erreurs API. L’interface `infrastructure::auth::{AuthConfig, Authenticator}` est réexportée par le module d’authentification.

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

Les deux applications partagent la même présentation d’en-tête : logo propre à l’application, nom et sous-titre à gauche, burger à droite et ligne orange Ni². Dans Catalog Services, le menu contient **Catalogue de services**, **Mes sélections**, **Paramètres** et **Déconnexion**, sur ordinateur comme sur mobile. Le header est neutre ; les fonds pâles, titres, badges et boutons métier restent bleus pour `users`, prune pour `admin`, sans bordure épaisse répétée en haut du bandeau. `/settings` regroupe les liens vers les fonctions natives de sécurité pour les deux rôles ; le lien vers la console d’administration n’apparaît que pour `admin`. Le retour des actions natives conserve la destination `/settings`. Les anciennes pages `/user` et `/admin` ne sont plus des écrans distincts : elles redirigent vers les paramètres, en conservant le contrôle `admin` sur l’ancienne URL `/admin`.

Le thème Login `ni2`, monté en lecture seule depuis `infrastructure/keycloak/themes/ni2`, hérite de `keycloak.v2` et applique uniquement des surcharges CSS Ni² (PatternFly 5). Aucun template ou script natif n’est remplacé : connexion, mots de passe et OTP conservent leur comportement. Le thème reste commun à tous les rôles. La distinction bleu utilisateur / prune administrateur est réservée à Vue. Les consoles Account et Administration ainsi que les e-mails gardent leurs thèmes natifs. Les caches de thèmes sont désactivés uniquement pour le développement local.

Les actions requises et flux d’authentification natifs sont ceux créés par Keycloak à l’import. Les quatre comptes locaux de démonstration n’ont ni changement de mot de passe initial imposé, ni OTP obligatoire. `demo-security` est réservé aux exercices de changement/récupération de mot de passe et d’OTP ; `demo-bruteforce` aux essais de blocage temporaire. Les mots de passe humains suivent une convention publique réservée à localhost, contrairement aux secrets techniques qui restent protégés et, pour AD, aléatoires.

Keycloak envoie son SMTP à `mailpit:1025`, sans authentification ni TLS dans le réseau Docker local. Aucune adresse SMTP externe n’est configurée et aucune option de relais n’est activée dans Mailpit. Les adresses fictives sont marquées vérifiées à l’initialisation pour éviter d’ajouter un parcours de vérification hors scope.

### Protection native contre les essais de mots de passe répétés

Le realm configure un blocage temporaire après cinq échecs de mot de passe,
avec une attente plafonnée à 60 secondes et sans verrouillage permanent.
Keycloak applique ce contrôle avant l’émission des jetons ; ni Vue ni Rust
n’implémente de compteur de tentatives. Les jetons et sessions existants ne sont
pas révoqués par ce mécanisme. Le parcours manuel, les valeurs exactes et les
limites sont décrits dans [le guide brute force](brute-force.md).

## 8. Reproductibilité et limites assumées

- Les versions des outils frontend sont fixées et `package-lock.json` est fourni ; Rust utilise `Cargo.lock` et les builds utilisent les modes verrouillés.
- Les images Keycloak et Mailpit ont une version explicite. Les images de compilation Node/Rust et de runtime Nginx/Debian restent des tags ; ce n’est pas une garantie de reproductibilité bit à bit.
- L’import au démarrage **ne remplace pas** un realm existant. La persistance permet de conserver les exercices réalisés.
- La base embarquée `dev-file` convient à une instance de démonstration, pas à un déploiement de production ou multi-instance.
- Les identifiants humains initiaux sont publics et réservés à l’environnement local. Les fichiers d’initialisation et Docker connaissent ces valeurs ; l’application Vue/Rust ne les reçoit pas. Le secret technique `catalog-loader`, généré séparément par Keycloak, n’est pas inscrit dans ces fichiers et reste dans un fichier local protégé pour l’outil appelant.
- La validation locale d’un JWT ne vérifie pas à chaque requête la session Keycloak : la révocation et les changements de rôles sont observés au renouvellement ou à l’expiration du jeton, pas immédiatement.
- Les tests backend utilisent de vraies signatures et un faux serveur de clés ; ils ne prouvent pas à eux seuls que l’import Keycloak est correct. Le guide de scénarios couvre cette intégration.
- Pas d’inscription libre, de politique OTP obligatoire, de formulaires IAM personnalisés, de refonte des templates, de BFF, de base applicative, d’introspection distante ou de déploiement public.

Évolutions possibles après V1 : tests navigateur pérennes, inscription native Keycloak avec rôle par défaut maîtrisé, personnalisation de la console Account et durcissement pour un véritable environnement cible. Aucune de ces évolutions n’est activée implicitement.

## 9. Catalogue : une architecture hexagonale légère

Le catalogue constitue une ressource métier fictive, volontairement distincte de l’IAM. Ajouter un service ou modifier un prix ne crée aucune infrastructure et ne modifie aucun utilisateur ni rôle Keycloak.

```text
Vue / curl / Postman
       │ HTTP + Bearer
       ▼
infrastructure::auth::authenticate  Validation JWT et traduction en Actor
       ▼
infrastructure/http/catalog_http.rs Adaptateur entrant : JSON, rôles, HTTP
       ▼
application/catalog_service.rs      Cas d’usage utilisant domain/catalog.rs
       │
       ▼
application/ports/CatalogRepository Port sortant défini côté cœur
       ▲
       │ implémenté par
infrastructure/persistence/          Adaptateur sortant : InMemoryStore
```

### Trois couches explicites

Le backend est organisé par couche, et non plus par dossiers métier mélangeant
le domaine, les cas d’usage et les adaptateurs :

```text
catalog-services/backend/src/
├── domain/
│   ├── catalog.rs
│   └── selection.rs
├── application/
│   ├── access.rs
│   ├── catalog_service.rs
│   ├── selection_service.rs
│   └── ports/
│       ├── catalog_repository.rs
│       └── selection_repository.rs
├── infrastructure/
│   ├── auth/          jwt.rs, jwks.rs
│   ├── http/          catalog_http.rs, selection_http.rs
│   ├── persistence/   in_memory_store.rs
│   └── seed/          demo_catalog.rs
├── lib.rs
└── main.rs
```

Les fichiers `mod.rs` déclarent les sous-modules de chaque couche. Le domaine
ne dépend que de la bibliothèque standard. L’application dépend du domaine et
de ses propres ports ; elle ne connaît ni Axum, ni Serde, ni JWT, ni le stockage
concret. L’infrastructure dépend de l’application et du domaine, jamais l’inverse.
Ces frontières sont organisées en modules dans un même crate, pas en crates
indépendants ; le compilateur n’interdit donc pas à lui seul de futurs imports
contraires à cette règle. Cette organisation concerne le backend de Catalog
Services, pas le frontend Vue ni Cost Monitor.

### Rôles des fichiers

| Fichier relatif à `catalog-services/backend/src/` | Responsabilité |
| --- | --- |
| `domain/catalog.rs` | Types `DatacenterService`, `ServiceDraft` et cinq catégories ; aucune dépendance à Axum, Serde, JWT ou Keycloak |
| `domain/selection.rs` | Sélection, propriétaire immuable, contrôle de propriété, nom et appartenance sans doublons |
| `application/access.rs` | `Actor` et permissions applicatives ; aucun nom de scope OAuth |
| `application/catalog_service.rs` | Lister, consulter, créer, modifier, supprimer et remplacer le catalogue |
| `application/selection_service.rs` | Cas d’usage personnels, capacités, contrôle de propriété, totaux et nouvelles tentatives après conflit |
| `application/ports/catalog_repository.rs` | Trait `CatalogRepository`, contrat de stockage synchrone `Send + Sync` |
| `application/ports/selection_repository.rs` | Trait `SelectionRepository`, instantanés cohérents et sauvegarde conditionnée par les versions |
| `infrastructure/http/` | Routes Axum, DTO JSON, contrôles de rôles et conversion des erreurs métier en HTTP |
| `infrastructure/auth/` | Validation JWT/JWKS et traduction rôles/scopes/client en permissions applicatives |
| `infrastructure/persistence/in_memory_store.rs` | Implémentation mémoire des deux ports de stockage |
| `infrastructure/seed/demo_catalog.rs` | Adaptateur de lecture du JSON prédéfini embarqué via `include_str!` |

### Assemblage : `lib.rs` et démarrage : `main.rs`

`lib.rs` déclare `domain`, `application` et `infrastructure`. C’est le point de
composition : `app(auth)` choisit et construit un `InMemoryStore`, l’injecte
dans `CatalogService` et `SelectionService`, puis appelle `app_with_services`
pour assembler les routes et le middleware. Les services ne choisissent pas
leur stockage ; ils reçoivent une implémentation de leurs ports.

`main.rs` lit l’environnement, initialise les logs et l’authentification,
charge les clés publiques, ouvre le port et lance Axum avec arrêt gracieux.
Il ne contient aucune règle métier. Les tests peuvent importer la bibliothèque
et construire le routeur sans démarrer ce point d’entrée ni le port de production.

L’adaptateur mémoire implémente les deux ports avec des `BTreeMap`, compteurs et
un même `Mutex`. Chaque construction de l’application commence avec un stockage neuf.

Le repository **implémente** le port dont dépend le service applicatif. Remplacer la mémoire par un autre stockage ne doit pas imposer de changement au domaine ni aux handlers. Il n’y a pas de trait par cas d’usage : les méthodes du service constituent son interface d’entrée.

### Sécurité à la frontière HTTP

Les routes du catalogue passent toutes par le middleware JWT existant. Après authentification, l’adaptateur HTTP applique les rôles : `users` ou `admin` pour les lectures, `admin` pour le CRUD, et exclusivement `catalog-seed` pour le chargement intégral. Il traduit les claims en `Actor` du cœur, qui contient l’identité et les capacités résultant de l’intersection rôles/scopes/plafond client. Le service applicatif exige aussi la capacité correspondant à l’opération ; un nouvel adaptateur ne peut pas contourner cette vérification en appelant directement le service. Le domaine et les services ne connaissent ni JWT ni Keycloak.

Les boutons Vue sont uniquement une aide visuelle, pas la protection réelle. Les erreurs métier (`ServiceNotFound`) n’ont pas de code HTTP dans le cœur ; l’adaptateur les convertit en `404`. Les corps JSON incompatibles et catégories inconnues donnent `400`, sans ajouter de règles métier de longueur ou d’unicité.

### État, concurrence et chargement

Le repository est partagé par toutes les requêtes du processus. Ses opérations synchrones utilisent de courtes sections critiques sans `.await`. Le remplacement intégral prend un seul verrou : une lecture ne peut pas observer un catalogue à moitié chargé. Les identifiants restent croissants pendant la vie du processus, y compris lors d’un rechargement, pour éviter de réaffecter à un nouveau service l’identifiant encore détenu par un ancien écran.

Le backend démarre vide. Seul `POST /api/catalog/seed`, avec un jeton portant `catalog-seed` obtenu par le client technique, charge les cinq services du fichier `catalog-services/backend/data/demo_catalog.json` et **remplace toutes les données courantes**. Le jeton d’un administrateur humain ne suffit plus. Le fichier est désérialisé avant le remplacement : une erreur de lecture ne détruit pas le catalogue courant. Il n’existe ni bouton de chargement, ni endpoint d’upload, ni URL externe configurable, ni nouvel accès à l’API d’administration Keycloak dans Rust.

Le prix est un entier de centimes dans le domaine et l’API. Vue le convertit pour le formulaire et l’affiche en euros. La page `/catalog` est commune : consultation pour tous les comptes applicatifs, formulaire d’ajout/modification dans la page et suppression avec confirmation pour les administrateurs. Le catalogue se charge à l’ouverture et après chaque modification locale. Les autres sessions voient les changements en quittant puis en rouvrant le catalogue ; aucun bouton d’actualisation ni mécanisme temps réel n’est ajouté.

Les tests applicatifs s’exécutent sans HTTP ni JWT ; les tests de l’adaptateur REST utilisent le même serveur JWKS local que les tests de sécurité. Un nouveau repository représente un redémarrage et doit toujours commencer vide. Le stockage en mémoire ne fournit ni durabilité, ni synchronisation entre plusieurs processus : c’est un choix explicite de démonstration.

## 10. Sélections personnelles : autorisation dans le cœur

Une sélection possède un nom, un propriétaire immuable (`OwnerId`) et un ensemble
d’identifiants de services sans doublons (`BTreeSet`). Le domaine définit
`Selection::ensure_owner`. Le service applicatif l’applique avant toute lecture
individuelle, modification ou suppression, et filtre les listes avec cette même
règle. Il ne connaît ni JWT, ni HTTP, ni rôle administrateur.

```text
HTTP : JWT validé → rôle users/admin → Actor (sub + capacités)
                            ↓
SelectionService : charger → règle du domaine → lire/modifier
                            ↓
SelectionRepository : instantanés cohérents et écritures conditionnelles
                            ↑
InMemoryStore : même état partagé pour catalogue et sélections
```

L’adaptateur HTTP contrôle les rôles, décode les DTO et convertit les erreurs.
Le service exige `SelectionsRead` ou `SelectionsWrite` avant l’opération puis
applique la règle de propriété avec `actor.owner()`. Une capacité absente donne
`403`, y compris pour un utilisateur administrateur représenté par Cost Monitor.
`AccessDenied` et `NotFound` deviennent le même `404` pour ne pas révéler une
sélection étrangère. Le corps JSON ne peut fixer ni transférer la propriété.
`admin` n’accorde aucune exception. Le `sub` suffit dans ce resource server qui
n’accepte qu’un issuer configuré ; une extension multi-issuer demanderait une
identité composée de l’issuer et du sujet.

Le port fournit un instantané de la sélection et du catalogue, accompagné de
versions. Après autorisation et modification par le cœur, la sauvegarde compare
atomiquement les versions de la sélection et du catalogue. En cas de conflit,
le service relit et réapplique la règle, avec huit tentatives maximum, puis une
erreur de conflit (`409`). Il n’y a aucun `.await` sous le verrou.

La suppression d’un service retire ses références dans le même état verrouillé,
comme une contrainte référentielle en base. Le seed nettoie toutes les anciennes
références. Une sauvegarde périmée ne peut ni les réintroduire, ni annuler une
modification concurrente. Les propriétaires restent immuables. **Ces garanties
de stockage ne remplacent pas la règle d’autorisation métier.** Un futur adapter
de base devra respecter le même contrat (transactions et écritures
conditionnelles), pas réinventer la politique d’accès.

Les prix ne sont pas copiés dans la sélection. Le service construit les détails
et le total avec un seul instantané, en centimes et avec `checked_add`. Un
dépassement donne une erreur explicite (`422`) ; une addition invalide n’est pas
enregistrée. Pas de quantité, de partage, de réservation ou de provisionnement.

Les tests du cœur s’exécutent sans serveur : un repository volontairement non
filtrant démontre que l’autorisation ne dépend pas de l’adaptateur. Les tests
HTTP signent des JWT avec plusieurs `sub`, à rôles identiques ou administrateurs,
et exercent chaque opération contre une sélection étrangère. Les comptes locaux
et AD suivent le même chemin. Keycloak Authorization Services, délégation à une
autre application et offline tokens sont traités séparément ci-dessous.

## 11. Cost Monitor : login applicatif et délégation catalogue distincts

L’identité connectée des deux applications est présentée discrètement dans le
header : pastille d’initiales et nom d’utilisateur sur ordinateur, pastille seule
sur petit écran et nom complet consultable dans le burger. Le libellé accessible
reste complet même quand le texte visuel est tronqué ou masqué. Les valeurs
viennent des sessions authentifiées, sans requête IAM supplémentaire ni affichage
du champ e-mail ou du sujet technique. Cost Monitor n’affiche plus de carte
« Bonjour… » ; seules une carte de connexion catalogue si nécessaire et les
données métier occupent son contenu principal.

Cost Monitor est un serveur Axum distinct, avec des pages HTML rendues côté
serveur et une session opaque dans le navigateur. Son premier client Keycloak,
`cost-monitor-login`, authentifie uniquement l’utilisateur (`openid profile`,
sans audience API ni offline). Ses jetons sont validés puis abandonnés : la
session locale ne conserve que l’identité. Le tableau de bord est utilisable
sans connexion catalogue.

Le bouton **Connecter mon catalogue** lance un second parcours avec le client
`cost-monitor`, consentement de lecture, `email` et `offline_access`. Cost Monitor ne lit
jamais le repository des sélections : il appelle leur API avec ce second access
token. Les réponses restent filtrées par la règle métier de propriété.

```text
Navigateur ── cookie de session opaque ──→ Cost Monitor :8082
    │                                        │
    ├── login Cost Monitor ──────────────→ Keycloak (cost-monitor-login)
    └── consentement catalogue ─────────→ Keycloak (cost-monitor)
Cost Monitor ── deux codes/PKCE/secrets distincts ──┘
Cost Monitor ── offline token catalogue → access token ──┘
Cost Monitor ── access token utilisateur ──→ API Rust /api/selections
Cost Monitor ── alertes métier SMTP ───────→ Mailpit:1025
```

Le module OIDC s’appuie sur `openidconnect` pour le protocole et la validation
des ID tokens. Le module `sessions` conserve uniquement les identités, CSRF et
délais des sessions navigateur. Le module `integrations` conserve séparément
les connexions catalogue, indexées par **issuer + sub**, sans dépendre d’une
session. Chaque compte possède un verrou de renouvellement commun à toutes ses
sessions, ainsi qu’un nonce, ses jetons, une expiration d’access token et un
compteur. Les deux stockages restent bornés et en mémoire. La fermeture ou
l’expiration d’une session ne supprime pas l’intégration : une nouvelle session
du même compte la retrouve, mais un autre compte ne peut pas la consulter.
Les deux secrets techniques viennent de fichiers protégés,
jamais du navigateur. L’adaptateur web gère cookies, CSRF, état OIDC et affichage.

Les callbacks et cookies temporaires des deux parcours sont distincts. Le
retour catalogue doit retrouver la session locale qui l’a initié et le même
issuer et `sub` que le login applicatif. La comparaison est portée par les modèles
de session et d’intégration, pas uniquement par le handler HTTP. Un compte
différent est refusé sans changer le login ou conserver ses jetons. Une génération
commune au compte est vérifiée avant et après l’échange réseau : une suppression
catalogue dans une session empêche un ancien callback d’une autre session de la
recréer. La session initiatrice doit aussi être encore valide : un logout annule
ses tentatives, sans effacer une intégration précédemment établie. Les générations
des emplacements ne sont pas recyclées pendant la vie du processus (128 comptes
maximum), même après suppression des jetons, pour éviter un problème ABA.

Le client `cost-monitor` demande `offline_access`, après consentement, et
`selections:read`. Une session SSO en ligne et l’autorisation offline ont des
cycles de vie distincts. Une déconnexion de la **SPA catalogue** n’annule pas
l’offline token ; une révocation du consentement client dans Keycloak bloque
les renouvellements suivants. Aucun jeton offline ne sert de credential direct
à l’API. **Déconnecter le catalogue** dans Cost Monitor efface cette connexion
mais conserve le login applicatif. **Se déconnecter de Cost Monitor** ferme la
session navigateur et annule ses tentatives de connexion, mais conserve
l’intégration du compte. Une suppression catalogue, elle, s’applique à toutes les
sessions de ce compte. Aucune action locale ne
prétend révoquer le consentement distant. La révocation catalogue rend cette
intégration inutilisable mais ne déconnecte pas l’utilisateur de Cost Monitor.
Le login utilise `prompt=login` (choix d’un autre compte via les formulaires
natifs), tandis que la connexion catalogue utilise `prompt=consent` et peut
réutiliser le SSO. Aucun backchannel logout applicatif n’est ajouté ici.

Les capacités du cœur ne sont pas des rôles Keycloak : `application::access::Actor` est
construit à la frontière depuis des claims signés, avec intersection des scopes,
rôles et permissions maximales du client `azp`. `cost-monitor` est plafonné à
`SelectionsRead`, `catalog-loader` à `CatalogSeed` et la SPA aux opérations
interactives autorisées par le rôle. `cost-monitor-login` n’a aucune capacité API,
même en cas de jeton mal configuré portant une audience ou des scopes excessifs.
Les clients/scopes absents ou inconnus
échouent en mode fermé. Le scope `offline_access` ne donne aucune capacité.

Le catalogue possède les services et sélections ; Cost Monitor possède les seuils
et l’état de notification dans l’intégration du compte, pas dans la session.
`alerts.rs` porte les règles et les montants exacts en centimes ; `catalog.rs`
partage la passerelle de lecture entre pages et contrôleur, et `mail.rs` adapte
le port `Mailer` à SMTP avec `lettre`. Le contrôleur de `web.rs` s’exécute toutes
les 60 secondes, avec huit comptes maximum en parallèle et sans cycles superposés.
Une notification est acquittée après succès SMTP et retentée en cas d’échec.
Après un dépassement notifié, un retour au seuil ou en dessous réarme l’alerte
et déclenche un e-mail unique de retour à la normale. Un retour non envoyé est
retenté tant que le coût reste normal, mais annulé si un nouveau dépassement
le rend obsolète. Aucun e-mail de retour n’est envoyé sans dépassement notifié.
Un changement de seuil réinitialise cet état. Un instantané échoué ne
change pas cet état. Le header de génération `x-catalog-instance` empêche la
réutilisation de seuils après un redémarrage du backend ; les règles des
sélections supprimées sont retirées à la prochaine lecture réussie.

Le destinataire vient exclusivement des claims d’un ID token catalogue validé.
Une adresse vérifiée est requise par défaut ; Compose contient une exception
explicite pour les adresses AD fictives capturées par Mailpit. Les notifications
métier ne passent jamais par l’administration Keycloak. Les cloches ouvrent des
modales natives avec un petit script externe de même origine, sans JavaScript
inline. Une déconnexion catalogue suspend les contrôles, mais conserve les seuils.
Un redémarrage de Cost Monitor perd tout cet état mémoire ; aucune garantie
durable de livraison exactement une fois n’est revendiquée.

Une nouvelle base de données, Keycloak Authorization Services et l’historique
restent hors périmètre. Les limites de
sécurité locales et le scénario reproductible sont décrits dans
[le guide Cost Monitor](cost-monitor.md).
