# Cost Monitor : délégation utilisateur et accès offline

Cost Monitor est une application **Vue / TypeScript / Vite + Rust / Axum**, accessible sur
<http://localhost:8082>. On se connecte d’abord à **Cost Monitor**, puis on
autorise séparément l’accès au **catalogue**. La deuxième connexion permet de
consulter les sélections personnelles et leurs coûts au nom du même utilisateur,
en lecture seule. Des seuils configurables par sélection déclenchent des alertes
e-mail, vérifiées chaque minute ; aucun historique de prix n’est conservé.

L’interface affiche l’utilisateur dans le header (initiales et nom d’utilisateur),
sans bloc « Bonjour… ». Un bouton de connexion catalogue reste mis en évidence
dans une carte dédiée si nécessaire ; une fois connecté, les sélections et les
coûts apparaissent directement. Le burger **Menu du compte**, en haut à droite,
regroupe **Se déconnecter de Cost Monitor**, **Renouveler maintenant** si le
catalogue est connecté et **Déconnecter le catalogue** si une intégration existe.
Le menu est un popover HTML natif (clavier, Échap, clic extérieur), intégré à Vue.
Les détails pédagogiques restent dans ce guide, pas dans le tableau de bord.
Sur petit écran, seule la pastille reste visible dans le header ; le nom complet
est consultable en tête du menu. Sur ordinateur, le nom est affiché dans le
header uniquement, tronqué si nécessaire. Son libellé accessible et son titre
conservent le texte complet. L’identité vient de la session OIDC authentifiée,
sans affichage du champ e-mail ou du `sub` technique.
Le logo et la favicon utilisent le même SVG dédié
`cost-monitor/frontend/public/logo.svg` : une baie de serveurs issue du symbole du
catalogue, associée à trois pièces empilées orange, dont la pièce supérieure
inclinée porte un symbole €. La palette reste bleu nuit et orange Ni².

## Installation ou mise à niveau

Depuis la racine, avec les comptes et volumes existants conservés :

```sh
docker compose up -d --wait keycloak mailpit
python3 infrastructure/keycloak/scripts/configure-cost-monitor.py
docker compose --profile cost-monitor up --build -d --wait
```

Si Samba AD a déjà été configuré, démarrer aussi son profil :

```sh
docker compose --profile ad --profile cost-monitor up -d --wait
```

Pour une nouvelle installation AD, suivre d’abord [son guide](active-directory.md).
Le helper de Cost Monitor configure le client public de login, le client confidentiel
de délégation catalogue, les scopes et les
correspondances de rôles via le CLI officiel Keycloak, sans réinitialiser le
realm, les comptes ou les mots de passe. Il peut être rejoué sans renouveler le
secrets. Si les identifiants bootstrap ont changé, fournir
`KEYCLOAK_ADMIN_USER` et `KEYCLOAK_ADMIN_PASSWORD` comme pour les autres helpers.

Le secret généré par Keycloak pour la délégation est synchronisé dans
`.secrets/cost-monitor.secret`, avec permissions 0600 et ignoré par Git.
`cost-monitor-login` est public : aucun secret n’est lu, exporté ou monté pour ce
client. Un ancien fichier `cost-monitor-login.secret` peut rester localement,
ignoré et inutilisé ; il n’est pas supprimé automatiquement. Aucun secret fixe
dans le realm JSON ou les images. L’import vierge définit les deux clients.

Pour mettre à jour uniquement Cost Monitor sans vider les sélections du backend :

```sh
python3 infrastructure/keycloak/scripts/configure-cost-monitor-spa.py
docker compose --profile cost-monitor up --build -d --no-deps --wait cost-monitor cost-monitor-frontend
```

Ce helper de migration ne modifie que les paramètres SPA du client de login,
en conservant son thème actuel, les autres attributs et les protocol mappers
non concernés. Il ne modifie pas la délégation ni les comptes.
Les intégrations, seuils, états de notification et tentatives mémoire de Cost
Monitor sont perdus à son redémarrage. Le login revient désormais sur `/` dans
Vue ; seul le callback de délégation `/oidc/catalog/callback` reste côté Rust.

**Lors de la mise à niveau historique des scopes catalogue**, se reconnecter à
la SPA : les anciens jetons sans scopes métier sont refusés (`403`). La migration
Vue seule ne nécessite pas de reconstruire le backend catalogue ni de le vider.

## Organisation Vue + Rust

Comme Catalog Services, l’application est organisée en `frontend/` et `backend/` :

```text
cost-monitor/
├── frontend/   Vue 3, TypeScript, Vite, Vue Router, composants et tests Playwright
└── backend/    Rust/Axum, OIDC, sessions, intégrations, alertes et SMTP
```

Le service Compose `cost-monitor-frontend` sert le build Vue via Nginx sur le port
public **8082** et relaie les API et parcours OIDC vers `cost-monitor:8082`, qui
n’est plus publié sur l’hôte. Les cookies et appels restent de même origine,
sans CORS permissif. La santé du serveur Rust reste disponible dans le réseau
Docker sur `/health`. Nginx conserve l’`Origin` du navigateur avec son port.
Le journal d’accès Nginx est désactivé pour les routes proxy, notamment pour ne
pas enregistrer les codes d’autorisation présents dans les callbacks OIDC.

Les deux SPA utilisent maintenant **`keycloak-js`**. Le client public
`cost-monitor-login` utilise Authorization Code + PKCE S256, une origine CORS
exacte, des retours `/` et `/silent-check-sso.html` exacts et aucun scope offline.
Ses jetons online (access, ID, refresh) restent uniquement en mémoire JavaScript,
jamais dans `localStorage` ou `sessionStorage`. L’access token a l’audience dédiée
`cost-monitor-api` et ne donne aucun accès à l’API catalogue.

Rust valide à chaque appel la signature RS256, issuer, audience, expiration,
éventuel `nbf`, sujet, `sid`, `typ=Bearer` et `azp=cost-monitor-login`. Un ID token,
un jeton catalogue, un ancien cookie de session ou un jeton d’un autre client ne
peut pas authentifier l’API. Il n’y a plus de login serveur ni de session par cookie.
La bibliothèque `openidconnect` reste utilisée uniquement pour la délégation catalogue.

| Route | Réponse / usage |
| --- | --- |
| `GET /api/dashboard` | Bearer requis, JSON du seul compte connecté ; sinon `401` |
| `POST /api/catalog/connect` | Bearer + origine exacte, URL de consentement et cookie temporaire de liaison |
| `GET /oidc/catalog/callback` | Validation état/cookie/TTL, mise en attente du code, aucun échange ni attachement |
| `POST /api/catalog/complete` | Bearer de la même identité et du même `sid`, échange et attachement après vérifications |
| `POST /api/catalog/refresh` | Bearer + origine, tableau de bord JSON après renouvellement forcé |
| `POST /api/catalog/disconnect` | Bearer + origine, déconnexion catalogue, `204` |
| `POST /api/logout` | Bearer + origine, annulation des tentatives, `204` ; ne déconnecte pas Keycloak à lui seul |
| `POST /api/alerts/{id}`, `POST /api/alerts/{id}/delete` | Bearer + origine, seuil ou suppression, `204` |

Vue envoie `Authorization: Bearer …` et `Accept: application/json`. Le seuil reste
URL-encodé, sans jeton CSRF de session : aucun cookie n’authentifie une mutation.
L’`Origin` exact est aussi obligatoire sur les POST. Les anciennes routes login,
logout et mutations sans `/api` ne sont plus des handlers Rust.
Les erreurs API gardent leur statut JSON ; une erreur du callback de navigation
revient sur Vue comme texte échappé, puis sa query est retirée de l’adresse.

Après le consentement, la SPA a perdu ses jetons en mémoire par la navigation :
`keycloak-js` restaure le login online par `check-sso`, puis appelle la complétion.
Si l’iframe silencieux est indisponible, un fallback par redirection est permis
sur ce retour uniquement. La petite page `/silent-check-sso.html` peut être
encapsulée par sa propre origine ; toutes les pages applicatives gardent une CSP
anti-encapsulation. Aucun offline token ne transite par cette page ou par Vue.

Les identifiants et montants `u64` sont sérialisés en chaînes décimales. Vue utilise
`BigInt` pour afficher les centimes et calculer les totaux sans perte de précision.
Les identités, noms de sélections et services sont échappés par l’interpolation Vue.

Pour développer localement, lancer Rust depuis `cost-monitor/backend/` avec
`cargo run --locked` (port **3002**, secrets lus dans `../../.secrets/`), puis
`npm ci` et `npm run dev` depuis `cost-monitor/frontend/` (port **8082**).
Vite relaie les routes backend vers `127.0.0.1:3002` ; garder Keycloak, l’API
catalogue et Mailpit démarrés. Pour SMTP local, publier temporairement le port
1025 de Mailpit avec une configuration Compose locale, ou fournir `SMTP_HOST`
et `SMTP_PORT` pointant vers un serveur SMTP de développement accessible.

## Scénario de démonstration

1. Dans le catalogue (<http://localhost:8080>), ajouter ou charger des services,
   puis créer une sélection avec `user1` (ou un compte local).
2. Ouvrir Cost Monitor, puis **Se connecter à Cost Monitor**. Keycloak réutilise
   la session SSO Keycloak existante, sans imposer de nouvelle authentification.
   Sans session SSO, le formulaire natif apparaît. Utiliser `user1`.
   Pour choisir un autre compte, se déconnecter puis se reconnecter.
   Seule l’API Cost Monitor est accessible à cette étape, pas le catalogue ni l’offline.
3. Lors de la première connexion (ou après suppression explicite de
   l’intégration), le tableau de bord met en évidence **Connecter mon catalogue**.
   Une reconnexion au même compte retrouve au contraire son intégration
   existante, sans nouveau consentement. Cliquer sur **Connecter mon catalogue**
   si aucune intégration n’existe. Keycloak peut
   réutiliser le SSO du même utilisateur ; un consentement explicite est demandé
   pour la lecture des sélections et l’accès hors ligne (`prompt=consent`).
4. Après vérification du même issuer et du même `sub`, Cost Monitor affiche les
   sélections du compte représenté et leurs coûts. Un compte différent est
   refusé, même si c’est un administrateur ; l’identité Cost Monitor ne change
   jamais et les nouveaux jetons ne sont pas conservés. En cas de refus,
   déconnecter Cost Monitor puis s’y reconnecter avec le compte souhaité avant
   de recommencer la connexion catalogue. Le consentement éventuellement déjà
   donné dans Keycloak n’est pas révoqué implicitement par ce refus local.
5. Dans l’onglet de l’application catalogue, cliquer sur **Déconnexion**. Ne pas
   déconnecter le catalogue **dans Cost Monitor** : ses jetons doivent être conservés.
6. Tant que l’access token online de la SPA reste valide, ouvrir
   **Menu du compte → Renouveler maintenant**. Rust échange l’offline token
   catalogue, pas le refresh token de la SPA. Après expiration du login online,
   l’interface demande une reconnexion, mais le contrôleur offline continue sans elle.
7. Pour observer le renouvellement automatique à la lecture, attendre plus d’une
   minute puis **Actualiser les coûts**. Une lecture ou un contrôle d’alerte
   déclenche le renouvellement si nécessaire.

### Configurer et démontrer une alerte

1. Cliquer sur la **cloche en contour** d’une sélection : une modale permet de
   saisir un seuil mensuel positif ou nul (deux décimales maximum, virgule ou point).
2. Enregistrer : la cloche devient **jaune et remplie**, avec le seuil affiché.
   Un nouveau clic permet de modifier le seuil ou de supprimer l’alerte.
   Échap, Annuler ou un clic hors de la modale la ferme sans enregistrer.
3. Dans le catalogue, avec un administrateur, augmenter le tarif d’un service
   pour que le total dépasse strictement le seuil. Au prochain contrôle
   (cadence de 60 secondes), consulter <http://localhost:8025> : l’e-mail
   contient la sélection, ses services et prix actuels, le total et le seuil.
4. Se déconnecter de Cost Monitor et du catalogue ne suspend pas la surveillance.
   Aucun nouvel e-mail n’est envoyé tant que le dépassement continue.
5. Ramener le total au seuil ou en dessous : au prochain contrôle, un e-mail
   **Retour à la normale** est envoyé, avec la sélection et ses prix actuels.
   Il n’est envoyé qu’après un dépassement déjà notifié, sans rappel tant que
   le total reste au seuil ou en dessous. Augmenter ensuite le prix au-dessus
   du seuil déclenche un nouvel e-mail de dépassement.

Créer ou changer un seuil déjà dépassé déclenche l’alerte au prochain contrôle.
Enregistrer une valeur inchangée ne réarme pas l’alerte. Un échec SMTP est retenté
au contrôle suivant ; seule une acceptation SMTP acquitte la notification.
Le retour à la normale réarme immédiatement l’alerte, même si son e-mail échoue.
Cet e-mail est retenté tant que le total reste au seuil ou en dessous ; un nouveau
dépassement annule un retour à la normale non envoyé devenu obsolète. Un changement
de seuil réinitialise l’épisode, sans e-mail de retour à la normale artificiel.
Ce n’est pas une garantie durable « exactement une fois » en cas de perte d’état.

Le destinataire provient uniquement du claim `email` d’un **ID token validé** de
la délégation catalogue (`openid profile email selections:read offline_access`).
Aucun champ de formulaire ne permet de choisir une adresse. Par défaut,
`email_verified=true` est exigé. Compose active explicitement
`ALLOW_UNVERIFIED_ALERT_EMAIL=true` pour les adresses AD fictives, capturées
uniquement par Mailpit. Hors de cette démo, ne pas conserver cette exception.
Après ajout du scope `email`, reconnecter le catalogue pour recevoir ces claims.
Une adresse absente ou non autorisée désactive l’enregistrement dans la modale.

L’adaptateur SMTP métier utilise `SMTP_HOST`, `SMTP_PORT` et `SMTP_FROM` ; Compose
le dirige vers `mailpit:1025`, sans TLS ni authentification, sur le réseau local.
Ces alertes ne passent pas par l’API d’administration ou les e-mails IAM Keycloak.

Les access tokens du **client de délégation catalogue seulement** durent 60 secondes pour rendre la
démo observable ; ceux de la SPA et de `catalog-loader` gardent leur durée de
cinq minutes. Le bouton de renouvellement force un échange sans attendre cette
expiration, mais ne prolonge pas artificiellement la validité d’un ancien JWT.

### Révoquer la délégation

Dans la console de compte Keycloak, ouvrir **Applications**, puis révoquer
l’accès de **Catalogue — délégation à Cost Monitor** (client `cost-monitor`,
libellés variables selon la langue). Si nécessaire,
se reconnecter à la console de compte : l’accès offline de Cost Monitor reste
indépendant de cette nouvelle session en ligne.

Une autre possibilité pour un administrateur du realm est la révocation du
consentement client de l’utilisateur via l’administration native/API Keycloak.
Ce n’est pas une simple suppression des sessions SSO en ligne.

Revenir dans Cost Monitor et choisir **Menu du compte → Renouveler maintenant** : l’échange
est refusé et les jetons catalogue locaux sont abandonnés. **La session Cost
Monitor reste ouverte** et propose **Reconnecter mon catalogue**. Un access token déjà émis pourrait néanmoins rester accepté par
une autre API qui le valide localement jusqu’à son expiration : **révocation du
renouvellement ne signifie pas révocation instantanée de chaque JWT**.

### Déconnexion catalogue et déconnexion SSO

- **Déconnecter le catalogue** : effacer les jetons et l’affichage de cette
  intégration **pour toutes les sessions du même compte**, invalider ses
  tentatives en cours, mais rester connecté à Cost Monitor. Les seuils restent
  enregistrés en mémoire ; les contrôles sont suspendus jusqu’à la reconnexion.
- **Se déconnecter de Cost Monitor** : annuler les tentatives catalogue via l’API,
  puis appeler `keycloak.logout()` avec l’ID token online en mémoire. Keycloak reçoit
  son `id_token_hint`, termine le SSO et revient à `http://localhost:8082/`, sans
  confirmation dans le parcours normal. L’intégration et son offline token restent
  rattachés au compte côté serveur ; la surveillance continue. Si l’API est en panne,
  l’annulation est tentée au mieux, mais le logout Keycloak n’est pas bloqué.

Les identifiants online et la délégation offline sont séparés :

```text
SPA keycloak-js (mémoire JavaScript)
└── access token + ID token + refresh token online

Contexte serveur de tentative (sid signé, borné par expiration JWT)
└── identité vérifiée + annulation + état/nonce/PKCE + cookie de liaison 5 min

Intégration du compte (issuer + sub)
├── access token + offline token + nonce + expiration + compteur
└── seuils par sélection + état de notification
```

Test simple : connecter `user1` et son catalogue, se déconnecter de Cost Monitor,
puis s’y reconnecter avec `user1`. Les sélections sont retrouvées sans cliquer
sur **Connecter mon catalogue**. Le compteur de renouvellement reste interne au
serveur et n’est plus affiché. Se connecter avec
`user2` montre uniquement l’intégration de `user2` (ou aucune s’il n’en a pas).
Après **Menu du compte → Déconnecter le catalogue**, une reconnexion avec `user1` ne doit plus
retrouver cette intégration. L’échec du renouvellement online vide le login Vue,
sans supprimer l’intégration offline.

Après logout Keycloak, le prochain clic sur **Se connecter** permet de saisir un
autre utilisateur. L’ID token hint est utilisé par la bibliothèque officielle,
comme pour Catalog Services ; il ne doit pas être enregistré dans les logs.
Si le navigateur abandonne la navigation avant que Keycloak traite le logout,
le SSO peut rester actif. Le consentement offline n’est jamais révoqué par ce logout.
Les autres SPA ouvertes ne sont pas fermées instantanément : un access token
signé peut rester valide jusqu’à son expiration. Un redémarrage perd les
intégrations et tentatives serveur, y compris les alertes, puisque tout est en mémoire.

Le contrôleur vérifie jusqu’à huit comptes en parallèle, sans chevauchement de
cycles. Une lecture échouée ne réarme ni n’acquitte une alerte. Un instantané
réussi supprime les règles des sélections supprimées. Le header
`x-catalog-instance` identifie la génération du backend : son changement efface
les anciens seuils pour qu’un identifiant numérique réutilisé ne les hérite pas.
Sans ce header, aucun contrôle ni nouvel enregistrement d’alerte n’est effectué.

La durée du login suit désormais les jetons et la session online Keycloak, pas
une session serveur locale de 24 heures. Aucun backchannel logout ni liste de
révocation globale des JWT n’est implémenté. Le logout annule localement le `sid`
initiateur et ses callbacks ; les autres sessions suivent leurs propres jetons.

## Qui autorise quoi ?

Les permissions effectives sont l’intersection des **rôles de l’utilisateur**,
des **scopes OAuth accordés** et du **plafond autorisé au client `azp`** :

| Client | Scopes métier configurés | Effet |
| --- | --- | --- |
| `olaninc-spa` | `catalog:read`, `catalog:write`, `selections:read`, `selections:write` | Gestion personnelle ; écriture catalogue uniquement si rôle `admin` |
| `cost-monitor-login` | Aucun (seulement `openid profile`, avec `basic` par défaut) | API Cost Monitor uniquement (`aud=cost-monitor-api`, `azp` vérifié), sans accès catalogue ou offline |
| `cost-monitor` | `selections:read` | Lecture de ses sélections, même si l’utilisateur est administrateur |
| `catalog-loader` | `catalog:seed` | Seed uniquement avec le rôle `catalog-seed` |

Un client inconnu, un `azp` absent ou des scopes absents n’accordent aucune
capacité. Le plafond du client empêche un jeton Cost Monitor d’écrire même si
des scopes excessifs étaient accordés par erreur. Le scope `offline_access`
ne donne **aucune** permission métier ; il permet le renouvellement offline.

Les claims vérifiés sont traduits en `Actor` avec une identité et des capacités
du cœur applicatif. Les services Rust vérifient les capacités avant leurs
opérations. Le service des sélections applique ensuite la règle de propriété
du domaine, quelle que soit l’origine locale/AD du compte. Une sélection
étrangère garde sa réponse `404` ; une opération non autorisée par les
capacités du client donne `403`. Aucun appel d’administration IAM dans Rust.

## Frontières de sécurité et limites locales

- Un client public online et un client confidentiel offline, tous deux en
  Authorization Code + PKCE S256 ; pas de password grant, service account ou flux implicite.
- `keycloak-js` gère login, logout et renouvellement online. Rust utilise
  `jsonwebtoken` pour valider les Bearer ; aucune autorisation ne repose sur un
  JWT seulement décodé dans Vue. Les jetons des deux audiences ne sont pas interchangeables.
- Pour la délégation, `openidconnect` valide signature, issuer, audience,
  expiration, nonce et `at_hash`. Le retour est lié à un état serveur et au cookie
  aléatoire `cm_catalog_login` (`HttpOnly`, `SameSite=Lax`, cinq minutes, `Secure`
  en HTTPS). Ce cookie ne donne aucun accès API et aucun login ; `cm_session` et
  `cm_app_login` ne sont plus utilisés comme credentials.
- Le callback GET ne fait que mettre le code en attente. La complétion POST exige
  le Bearer de la même identité, du même `sid` et le même contexte de tentative.
  La génération de l’intégration et l’annulation sont revérifiées après l’échange
  réseau. Un compte différent, un logout ou une déconnexion catalogue ne peuvent
  pas être contournés par un callback tardif ou rejoué. La fermeture du login
  ne détruit pas une intégration déjà établie.
- L’intégration est indexée par **issuer + sub**, jamais par un propriétaire
  choisi par Vue. Les tokens catalogue/offline restent uniquement en mémoire
  Rust ; les tokens online restent uniquement en mémoire JavaScript. Aucun des
  deux n’est stocké dans le navigateur de façon persistante ou dans les logs applicatifs.
- Les API exigent un Bearer explicite ; les cookies seuls sont refusés. Les POST
  imposent aussi une origine exacte, sans ancien CSRF lié à un cookie de login.
  Pages et JSON non mis en cache, interpolation Vue échappée et CSP sans scripts
  ni styles inline. `connect-src` et `frame-src` autorisent uniquement l’origine
  Keycloak locale en plus de `self` ; seule la page de vérification silencieuse
  peut être encapsulée par `self`. Les modales restent des `<dialog>` natifs.
  HTTP et les comptes publics sont réservés à cette démo locale.
- Renouvellement sérialisé par **compte**, partagé entre toutes ses sessions ; conservation du nouveau refresh token
  si rotation, abandon local après `invalid_grant`. Une panne temporaire de
  Keycloak ne détruit pas l’autorisation sauvegardée.
- Contextes d’annulation : 128 `sid` maximum, bornés par l’expiration du JWT vérifié,
  et 128 tentatives de délégation de cinq minutes. Les contextes ne remplacent
  jamais la validation Bearer, y compris après un renouvellement du token.
- Registre d’intégrations : 128 comptes distincts maximum par processus. Il n’a
  pas de TTL lié aux sessions navigateur. Après suppression, un emplacement
  sans jetons conserve uniquement la clé et la génération jusqu’au redémarrage,
  pour empêcher un ancien callback de recréer l’intégration. Un compte existant
  peut toujours réutiliser son emplacement. C’est une limite explicite de cette
  petite démonstration mémoire.
- Le seul secret du client de délégation est monté en lecture seule, copié avec permissions 0600 dans
  un tmpfs par le bootstrap du conteneur, puis le processus tourne comme
  utilisateur non privilégié. Les tokens ne sont pas copiés dans ces fichiers.
- Les appels OIDC serveur réécrivent uniquement le transport de l’issuer public
  vers le service Docker Keycloak autorisé. La validation de l’issuer public
  reste active et aucun redirect HTTP sortant n’est suivi.
- La discovery et les clés OIDC sont chargées au démarrage : après une rotation
  non compatible des clés Keycloak, redémarrer Cost Monitor (ce qui perd les
  sessions **et intégrations** locales). Pas de coffre de secrets ou de stockage durable dans cette
  démo, ni de rechargement automatique des clés.
- Le total est une somme des sélections, pas une facture consolidée : un service
  présent dans deux sélections est compté dans chacune. Aucun prix historique.

## Vérifications

```sh
cargo test --locked --manifest-path catalog-services/backend/Cargo.toml
cargo clippy --locked --manifest-path catalog-services/backend/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path cost-monitor/backend/Cargo.toml
cargo clippy --locked --manifest-path cost-monitor/backend/Cargo.toml --all-targets -- -D warnings
cd cost-monitor/frontend
npm ci
npm run build
npx playwright install chromium
npm test
```

Les tests de l’API vérifient les scopes manquants, clients inconnus, accès croisés,
refus de toutes les écritures pour Cost Monitor et contrôle des capacités dans
les services, sans dépendre uniquement de HTTP. Ceux de Cost Monitor couvrent les
callbacks forgés/rejoués, PKCE, scopes offline requis, rotation, expiration,
validation cryptographique Bearer/audience/azp, refus des cookies seuls, origine exacte,
contrat JSON, montants exacts, absence de jetons offline dans les réponses, séparation
des logins, identités incompatibles, annulation des callbacks et conservation du
login applicatif après déconnexion/révocation catalogue, restauration de
l’intégration après logout/expiration de session, isolation par issuer et sub,
suppression globale au compte et rotation concurrente partagée entre sessions.
Les tests d’alerte couvrent les montants exacts, le dépassement strict, le
réarmement, le retour à la normale (égalité incluse), l’absence de doublons,
les échecs de lecture/SMTP, le contrôle après
logout, la politique de destinataire et le nettoyage après suppression/redémarrage.
Les tests navigateur utilisent le vrai `keycloak-js` avec un fournisseur OIDC et
des réponses API simulés : accueil, échappement,
montants, menu, modale clavier/mobile, e-mail requis, mutations, expiration de
session, restauration après consentement, ID token hint et logout malgré panne API.
Ils ne remplacent pas le test réel du SSO et des alertes avec
Keycloak, l’API catalogue et Mailpit.

Le test réel optionnel, depuis `cost-monitor/frontend/`, est :

```sh
npm run test:live
```

Il nécessite les services locaux sur leurs ports par défaut et les comptes AD
de démonstration `admin1`, `user4` et `user2`. Il crée ses propres service et sélection,
vérifie le SSO, le logout sans confirmation, les mutations Bearer, les audiences
séparées, le refus des cookies seuls, l’origine exacte, le dépassement après logout,
le renouvellement offline, la restauration du seuil, le retour à la normale et
l’isolation entre comptes, puis supprime ses ressources temporaires. Prévoir
environ deux minutes pour les cycles du contrôleur. Les e-mails de test restent
visibles dans Mailpit ; aucun consentement existant n’est révoqué.

Références officielles : [Keycloak Offline access](https://www.keycloak.org/docs/26.8.1/server_admin/#_offline-access),
[client scopes](https://www.keycloak.org/docs/26.8.1/server_admin/#_client_scopes).
