# Cost Monitor : délégation utilisateur et accès offline

Cost Monitor est une application Rust/Axum séparée, accessible sur
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
Le menu est un popover HTML natif (clavier, Échap, clic extérieur), sans JavaScript.
Les détails pédagogiques restent dans ce guide, pas dans le tableau de bord.
Sur petit écran, seule la pastille reste visible dans le header ; le nom complet
est consultable en tête du menu. Sur ordinateur, le nom est affiché dans le
header uniquement, tronqué si nécessaire. Son libellé accessible et son titre
conservent le texte complet. L’identité vient de la session OIDC authentifiée,
sans affichage du champ e-mail ou du `sub` technique.
Le logo et la favicon utilisent le même SVG dédié
`cost-monitor/resources/logo.svg` : une baie de serveurs issue du symbole du
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
Le helper de Cost Monitor configure les deux clients confidentiels, les scopes et les
correspondances de rôles via le CLI officiel Keycloak, sans réinitialiser le
realm, les comptes ou les mots de passe. Il peut être rejoué sans renouveler le
secrets. Si les identifiants bootstrap ont changé, fournir
`KEYCLOAK_ADMIN_USER` et `KEYCLOAK_ADMIN_PASSWORD` comme pour les autres helpers.

Les secrets générés par Keycloak sont synchronisés dans
`.secrets/cost-monitor-login.secret` (connexion applicative) et
`.secrets/cost-monitor.secret` (délégation catalogue), avec permissions 0600 et
ignorés par Git. Aucun secret fixe dans le realm JSON ou les images. L’import
vierge définit les deux clients ; le helper récupère les secrets et met à jour
les installations existantes sans rotation de celui du client catalogue.

Pour mettre à jour uniquement Cost Monitor sans vider les sélections du backend :

```sh
python3 infrastructure/keycloak/scripts/configure-cost-monitor.py
docker compose --profile cost-monitor up --build -d --no-deps --wait cost-monitor
```

Les sessions, intégrations, seuils et états de notification mémoire de Cost Monitor
sont perdus à son redémarrage.
L’ancien callback unique `/oidc/callback` est remplacé par deux callbacks exacts.

**Se déconnecter puis se reconnecter à la SPA après cette mise à niveau** :
les anciens jetons sans scopes métier sont refusés (`403`). La validation de
signature/issuer/audience/expiration ne change pas. La reconstruction du backend
vide son stockage en mémoire, donc le catalogue et les sélections.

## Scénario de démonstration

1. Dans le catalogue (<http://localhost:8080>), ajouter ou charger des services,
   puis créer une sélection avec `user1` (ou un compte local).
2. Ouvrir Cost Monitor, puis **Se connecter à Cost Monitor**. Keycloak demande
   une authentification (`prompt=login`) pour permettre de choisir le compte,
   même si un cookie SSO subsiste après une déconnexion locale. Utiliser `user1`.
   Si Keycloak affiche le compte précédent sans champ utilisateur modifiable,
   utiliser son bouton **Recommencer la connexion** pour choisir un autre compte.
   Aucun accès API ni accès offline n’est demandé à cette étape.
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
6. Dans Cost Monitor, ouvrir **Menu du compte → Renouveler maintenant**.
   Les données restent accessibles : le serveur a échangé l’offline token
   auprès de Keycloak, sans nouvelle connexion interactive.
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

### Deux déconnexions locales distinctes

- **Déconnecter le catalogue** : effacer les jetons et l’affichage de cette
  intégration **pour toutes les sessions du même compte**, invalider ses
  tentatives en cours, mais rester connecté à Cost Monitor. Les seuils restent
  enregistrés en mémoire ; les contrôles sont suspendus jusqu’à la reconnexion.
- **Se déconnecter de Cost Monitor** : fermer uniquement la session navigateur
  et annuler ses autorisations en cours. L’intégration déjà établie et son
  offline token restent rattachés au compte côté serveur.

La session et l’intégration utilisent deux stockages mémoire distincts :

```text
Session navigateur (identifiant aléatoire)
└── identité authentifiée + CSRF + expiration locale

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
retrouver cette intégration. L’expiration de la session navigateur a les mêmes
effets qu’une déconnexion, sans supprimer les jetons du compte.

Aucune de ces actions ne prétend fermer le SSO global ou révoquer le consentement
offline distant. La prochaine connexion à Cost Monitor redemande une
authentification et permet donc de saisir un autre utilisateur sans devoir se
déconnecter au préalable depuis le catalogue. Un redémarrage perd les sessions
et intégrations locales, y compris leurs alertes, puisque tout est en mémoire.

Le contrôleur vérifie jusqu’à huit comptes en parallèle, sans chevauchement de
cycles. Une lecture échouée ne réarme ni n’acquitte une alerte. Un instantané
réussi supprime les règles des sélections supprimées. Le header
`x-catalog-instance` identifie la génération du backend : son changement efface
les anciens seuils pour qu’un identifiant numérique réutilisé ne les hérite pas.
Sans ce header, aucun contrôle ni nouvel enregistrement d’alerte n’est effectué.

La session applicative locale a une durée propre de 24 heures ; elle n’est pas
automatiquement fermée par la déconnexion SSO d’une autre application. Aucun
backchannel logout du client de connexion n’est implémenté dans cette démo.

## Qui autorise quoi ?

Les permissions effectives sont l’intersection des **rôles de l’utilisateur**,
des **scopes OAuth accordés** et du **plafond autorisé au client `azp`** :

| Client | Scopes métier configurés | Effet |
| --- | --- | --- |
| `olaninc-spa` | `catalog:read`, `catalog:write`, `selections:read`, `selections:write` | Gestion personnelle ; écriture catalogue uniquement si rôle `admin` |
| `cost-monitor-login` | Aucun (seulement `openid profile`, avec `basic` par défaut) | Authentification à Cost Monitor, sans audience API, offline ou accès aux sélections |
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

- Deux clients confidentiels, Authorization Code + PKCE S256 ; pas de password grant,
  de service account ou de flux implicite pour Cost Monitor.
- Bibliothèque `openidconnect` : discovery, validation de signature, issuer,
  audience, expiration, nonce et `at_hash` lorsqu’il est présent. Les retours
  OIDC sont liés à un état aléatoire conservé côté serveur et à un cookie du
  navigateur ; les tentatives expirent après cinq minutes et ne sont pas rejouables.
- Les callbacks `/oidc/login/callback` et `/oidc/catalog/callback` et leurs
  cookies temporaires sont distincts. Une tentative catalogue est liée à la
  session locale et à une génération de connexion. L’issuer et le `sub` sont
  comparés avant toute conservation des jetons. La fermeture/expiration de la
  session initiatrice invalide ses callbacks tardifs, sans toucher à une
  intégration déjà établie. Une suppression catalogue invalide les tentatives
  de toutes les sessions du compte, y compris pendant un échange réseau déjà commencé.
- Les jetons du login applicatif sont validés puis abandonnés : la session
  conserve uniquement l’identité et la protection de session. Un registre séparé,
  indexé par **issuer + sub**, conserve l’intégration du compte, indépendamment
  de ses sessions. Aucune API ne permet de choisir arbitrairement ce propriétaire.
  La révocation de la délégation ne ferme pas le login et son état reste visible
  lors des prochaines connexions du compte.
- Access/offline tokens conservés uniquement en mémoire serveur, jamais dans
  le HTML, JavaScript, localStorage, les URLs du navigateur ou les logs.
- Cookie de session opaque aléatoire 256 bits, `HttpOnly`, `SameSite=Lax` ;
  `Secure` automatiquement en HTTPS. Ce projet reste en HTTP **local seulement**.
- Actions POST protégées par origine exacte et jeton CSRF ; pages non mises en
  cache, HTML échappé, CSP limitée aux scripts de même origine et protection
  contre l’encapsulation. Le petit contrôleur externe `/dialogs.js` manipule les
  modales natives `<dialog>` ; aucun script inline n’est autorisé.
  `Referrer-Policy: strict-origin` évite de transmettre le code OIDC dans un
  referer tout en préservant l’en-tête Origin des formulaires.
  La CSP autorise les formulaires uniquement vers Cost Monitor et l’origine
  Keycloak configurée : Chromium applique cette règle aussi à la redirection
  après le POST protégé de connexion catalogue.
- Renouvellement sérialisé par **compte**, partagé entre toutes ses sessions ; conservation du nouveau refresh token
  si rotation, abandon local après `invalid_grant`. Une panne temporaire de
  Keycloak ne détruit pas l’autorisation sauvegardée.
- Sessions locales : 24 heures maximum, 128 sessions et 128 connexions en cours.
  Cette limite locale est distincte des délais de validité offline de Keycloak.
  Les entrées expirées sont nettoyées lors des accès/nouvelles connexions.
- Registre d’intégrations : 128 comptes distincts maximum par processus. Il n’a
  pas de TTL lié aux sessions navigateur. Après suppression, un emplacement
  sans jetons conserve uniquement la clé et la génération jusqu’au redémarrage,
  pour empêcher un ancien callback de recréer l’intégration. Un compte existant
  peut toujours réutiliser son emplacement. C’est une limite explicite de cette
  petite démonstration mémoire.
- Les deux secrets clients sont montés en lecture seule, copiés avec permissions 0600 dans
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
cargo test --locked --manifest-path cost-monitor/Cargo.toml
cargo clippy --locked --manifest-path cost-monitor/Cargo.toml --all-targets -- -D warnings
```

Les tests de l’API vérifient les scopes manquants, clients inconnus, accès croisés,
refus de toutes les écritures pour Cost Monitor et contrôle des capacités dans
les services, sans dépendre uniquement de HTTP. Ceux de Cost Monitor couvrent les
callbacks forgés/rejoués, PKCE, scopes offline requis, rotation, expiration,
protection CSRF/origine, échappement, absence de jetons dans le HTML, séparation
des logins, identités incompatibles, annulation des callbacks et conservation du
login applicatif après déconnexion/révocation catalogue, restauration de
l’intégration après logout/expiration de session, isolation par issuer et sub,
suppression globale au compte et rotation concurrente partagée entre sessions.
Les tests d’alerte couvrent les montants exacts, le dépassement strict, le
réarmement, le retour à la normale (égalité incluse), l’absence de doublons,
les échecs de lecture/SMTP, le contrôle après
logout, la politique de destinataire et le nettoyage après suppression/redémarrage.

Références officielles : [Keycloak Offline access](https://www.keycloak.org/docs/26.8.1/server_admin/#_offline-access),
[client scopes](https://www.keycloak.org/docs/26.8.1/server_admin/#_client_scopes).
