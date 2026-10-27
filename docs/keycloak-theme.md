# Thème Ni² : connexion, profil et administration Keycloak

Le thème `ni2` personnalise les trois espaces natifs de Keycloak **26.8.1** :

| Type | Héritage | Présentation |
| --- | --- | --- |
| Login | `keycloak.v2` | Formulaires natifs de connexion, mots de passe et OTP |
| Account | `keycloak.v3` | Profil, sécurité, sessions et applications, accents bleus |
| Admin | `keycloak.v2` | Administration du realm, accents prune |

Les consoles gardent leurs applications React et leurs composants PatternFly 5.
Le thème ajoute seulement CSS, logos et favicons ; il ne contient aucun template
FreeMarker ou JavaScript de remplacement. Les polices sont celles disponibles
sur la machine, sans téléchargement externe. Le rendu est clair, même si le
navigateur utilise une préférence sombre (`darkMode=false`).

Le symbole de deux racks est une illustration de démonstration, pas le logo
officiel Ni². Les logos des consoles portent respectivement « Mon compte
Keycloak » et « Administration du realm ». L’en-tête blanc garde la ligne orange
commune aux applications. Les contrôles dangereux, désactivés, erreurs, dialogues
et QR codes restent gérés par les composants natifs.

## Montage dans le conteneur

Le montage est **déjà présent** dans `compose.yaml` :

```yaml
services:
  keycloak:
    volumes:
      - ./infrastructure/keycloak/themes/ni2:/opt/keycloak/themes/ni2:ro
```

Pour utiliser le thème dans un autre déploiement, copier le dossier entier et le
monter au même emplacement, en lecture seule. Ajouter ce montage à ceux du
déploiement existant, **sans remplacer ni supprimer le volume des données**.
Un nouveau montage nécessite de recréer le conteneur ; modifier les ressources
d’un montage déjà actif ne nécessite pas de reconstruire les applications Rust.

```text
infrastructure/keycloak/themes/ni2/
├── login/
│   ├── theme.properties
│   └── resources/...
├── account/
│   ├── theme.properties
│   └── resources/
│       ├── css/ni2.css
│       └── img/        logo.svg, favicon.svg
└── admin/
    ├── theme.properties
    └── resources/
        ├── css/ni2.css
        └── img/        logo.svg, favicon.svg
```

Les deux fichiers CSS sont indépendants afin que chaque type de thème référence
uniquement ses propres ressources. Maintenir les règles communes en cohérence
lors d’une évolution, tout en conservant les accents bleu/prune.

## Activer sur le realm existant

L’import JSON sélectionne les thèmes pour une installation vierge. Pour mettre
à jour le realm existant, démarrer Keycloak puis lancer depuis la racine :

```sh
python3 infrastructure/keycloak/scripts/configure-theme.py
```

Ce helper utilise le CLI officiel Keycloak, vérifie que les trois types sont
disponibles, modifie uniquement `loginTheme`, `accountTheme` et `adminTheme` dans
`olaninc`, puis relit ces valeurs. Il n’efface aucun compte, mot de passe, OTP,
client ou session. Les identifiants d’administration alternatifs utilisent
`KEYCLOAK_ADMIN_USER` et `KEYCLOAK_ADMIN_PASSWORD`, comme les autres helpers.

Activation manuelle possible : **Realm settings → Themes**, sélectionner `ni2`
pour **Login theme**, **Account theme** et **Admin theme**, puis enregistrer.
Recharger les consoles, éventuellement avec un rechargement forcé pour la favicon.

## Ouvrir les consoles

- Profil : <http://localhost:8081/realms/olaninc/account/>, par exemple avec
  `demo-user` / `demo-user-12345!`.
- Administration : <http://localhost:8081/admin/olaninc/console/>, avec
  `demo-admin` / `demo-admin-12345!`.

Le thème Admin ne donne aucun privilège : `admin1` reste un administrateur métier
sans accès à l’administration Keycloak. Le thème est celui du realm de la
console ; il n’est pas choisi selon le rôle de la personne connectée.
La console <http://localhost:8081/admin/master/console/> conserve son thème natif,
même si un administrateur de `master` y sélectionne ensuite le realm `olaninc`.
Pour voir la personnalisation, utiliser l’URL directe de la console `olaninc`.

## Revenir aux thèmes natifs

Dans **Realm settings → Themes**, sélectionner :

- Login : `keycloak.v2` ;
- Account : `keycloak.v3` ;
- Admin : `keycloak.v2`.

Cette opération ne réinitialise ni les comptes ni les données. Le thème e-mail
n’est pas modifié par le helper Ni².

## Vérifications et maintenance

```sh
python3 -m unittest discover -s infrastructure/keycloak/scripts/tests
```

Les tests vérifient l’héritage, les ressources référencées, les SVG, l’absence
de templates/scripts personnalisés et la portée limitée du helper.
Pour le rendu, vérifier sur ordinateur et mobile : profil, connexion/sécurité,
sessions, applications, navigation d’administration, recherche d’utilisateurs,
menus, focus clavier, contrastes et absence de débordement horizontal.

Après chaque mise à jour de Keycloak, contrôler l’existence des parents, les
classes et variables CSS PatternFly, les chargements des bundles hérités et les
parcours natifs. Les styles sont ciblés sur la version 26.8.1, pas garantis pour
toutes les futures versions. Les caches de thèmes sont désactivés dans Compose
pour la démo seulement ; un environnement de production doit utiliser sa propre
stratégie de cache et de déploiement.
