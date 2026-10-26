# Comptes et scénarios de démonstration

Les comptes humains suivent la convention **`<identifiant>-12345!`**.
Ces mots de passe sont volontairement publics et prévisibles : **localhost
uniquement, jamais des comptes ou des données réels**. Ils satisfont la politique
de longueur sans imposer un changement lors de la première connexion.

## Répartition des rôles

| Compte | Origine | Mot de passe de démonstration | Usage |
| --- | --- | --- | --- |
| `demo-user` | Keycloak local | `demo-user-12345!` | Consultation du catalogue, rôle `users` et sélections personnelles |
| `demo-admin` | Keycloak local | `demo-admin-12345!` | Gestion du catalogue (`admin`) et administration du realm (`realm-admin`) |
| `demo-security` | Keycloak local | `demo-security-12345!` | Changement de mot de passe, récupération par e-mail et OTP |
| `demo-bruteforce` | Keycloak local | `demo-bruteforce-12345!` | Cinq échecs espacés de deux secondes, blocage puis connexion après 65 secondes |
| `user1` | Samba AD | `user1-12345!` | Fédération, Cost Monitor, consentement, offline et alertes |
| `user2` | Samba AD | `user2-12345!` | Second propriétaire : prouver l’isolation des sélections |
| `user3` | Samba AD | `user3-12345!` | Login Cost Monitor puis délégation avec `user1` : identités incompatibles refusées |
| `user4` | Samba AD | `user4-12345!` | Réserve et démonstration d’un changement de groupe AD |
| `admin1` | Samba AD | `admin1-12345!` | Gestion des tarifs sans administration Keycloak/AD ; ne contourne pas la propriété des sélections |

Tous les comptes humains sauf `demo-admin` et `admin1` ont le rôle applicatif
`users`. Ces deux derniers ont `admin`. Seul `demo-admin` possède aussi
`realm-management:realm-admin`. Une connexion à Cost Monitor ne crée pas une
autre personne : utiliser le même compte pour son login et la délégation catalogue.

## Parcours conseillé

1. `demo-admin` crée des services ; `demo-user` peut les lire, mais pas les modifier.
2. `user1` et `user2`, dans des profils navigateur séparés, créent chacun une
   sélection. Même `admin1` ne peut pas consulter une sélection étrangère.
3. `user1` connecte Cost Monitor et configure un seuil. `admin1` change les
   tarifs : e-mail de dépassement, puis e-mail de retour à la normale.
4. Se déconnecter des applications sans retirer la délégation : les contrôles
   offline continuent. Révoquer ensuite l’accès catalogue dans Keycloak.
5. Pour la mauvaise identité, connecter Cost Monitor avec `user3`, puis choisir
   `user1` lors de la connexion catalogue. Si le SSO sélectionne automatiquement
   `user3`, utiliser un autre onglet pour changer la session SSO Keycloak, sans
   fermer la session applicative Cost Monitor, puis recommencer la délégation.
6. Réserver les exercices qui modifient les credentials à `demo-security`.
7. Réserver les essais de blocage à `demo-bruteforce` ; ne pas bloquer les comptes
   administrateurs ou AD pour cette démonstration.

## Installation et mise à niveau

L’import du realm crée les quatre comptes locaux uniquement sur un realm vierge.
Pour un realm déjà présent, avec Keycloak démarré :

```sh
python3 infrastructure/keycloak/scripts/configure-demo-users.py
```

Cette commande ajoute les comptes manquants et leurs rôles, mais ne réinitialise
pas les mots de passe existants. Elle refuse une collision avec un compte fédéré.
Pour revenir explicitement aux quatre mots de passe locaux de démonstration :

```sh
python3 infrastructure/keycloak/scripts/configure-demo-users.py --reset-passwords
```

Cette réinitialisation conserve les identifiants `sub`, les OTP existants et les
actions requises autres que `UPDATE_PASSWORD`. Elle ne révoque pas les sessions
ou les JWT existants. Après un exercice d’OTP, utiliser l’authentificateur associé
ou le supprimer via la console native ; le script ne le supprime jamais.

Pour les comptes AD existants, avec Samba démarré :

```sh
python3 infrastructure/active-directory/scripts/set-demo-passwords.py
```

Les identifiants `objectGUID`, groupes et secrets techniques sont conservés.
L’application Vue et les backends Rust ne reçoivent jamais ces mots de passe.

## Comptes techniques : ne pas les utiliser dans le catalogue

- `catalog-loader` : client OAuth de chargement, rôle `catalog-seed`, flux
  Client Credentials. Son secret reste dans `.secrets/catalog-loader.secret`.
- `bootstrap-admin` : maintenance Keycloak dans `master`, inchangé par ces scripts.
- `Administrator` : administration Samba, secret aléatoire et inchangé.
- `svc-keycloak` : liaison LDAP en lecture seule, secret aléatoire et inchangé.

Le fichier `.secrets/ad-credentials.json` contient aussi les secrets techniques :
ne pas le publier, même si les cinq mots de passe humains sont documentés ici.
