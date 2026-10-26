# Protection contre les tentatives répétées de connexion

Cette démonstration utilise uniquement la protection native de **Keycloak
26.8.1**. Aucun formulaire, compteur ou blocage n’est ajouté dans Vue ou Rust.
Les essais se font manuellement dans le navigateur, sur l’environnement local.

## Configuration

Dans la console d’administration, sélectionner le realm **olaninc**, puis
**Realm settings → Security defenses → Brute force detection** :

| Paramètre | Valeur de démonstration |
| --- | --- |
| Brute Force Mode | Lockout temporarily |
| Max Login Failures | 5 |
| Strategy to increase wait time | Multiple |
| Wait Increment | 1 minute |
| Max Wait | 1 minute |
| Failure Reset Time | 15 minutes |
| Quick Login Check Milliseconds | 1000 |
| Minimum Quick Login Wait | 1 minute |

Cliquer sur **Save**. Ces réglages s’appliquent au realm entier, donc à la
connexion au catalogue, à Cost Monitor et aux autres clients de `olaninc`.
Ils ne changent pas les règles du realm `master`.

Les mêmes valeurs figurent dans
`infrastructure/keycloak/olaninc-realm.json` pour une installation vierge :

```json
{
  "bruteForceProtected": true,
  "permanentLockout": false,
  "bruteForceStrategy": "MULTIPLE",
  "failureFactor": 5,
  "waitIncrementSeconds": 60,
  "maxFailureWaitSeconds": 60,
  "maxDeltaTimeSeconds": 900,
  "quickLoginCheckMilliSeconds": 1000,
  "minimumQuickLoginWaitSeconds": 60
}
```

**Sur une installation existante, modifier le JSON puis redémarrer ne suffit
pas** : l’import ne remplace pas le realm conservé. Appliquer ces valeurs dans
la console native, sans effacer les volumes, les comptes ou les mots de passe.
Pas de reconstruction des applications nécessaire.

## Démonstration dans le navigateur

1. Utiliser le compte local **demo-bruteforce**, pas un administrateur ni un compte
   Active Directory : l’annuaire peut avoir sa propre politique de verrouillage.
   Vérifier d’abord une connexion réussie avec son **mot de passe actuel**,
   terminer les éventuelles actions requises, puis se déconnecter. Cette réussite
   remet à zéro le compteur d’échecs de mot de passe.
2. Ouvrir <http://localhost:8080> dans une fenêtre privée et cliquer sur
   **Se connecter**. S’assurer que le formulaire Keycloak demande effectivement
   le mot de passe, sans réutilisation d’une session SSO.
3. Saisir `demo-bruteforce` et un mot de passe incorrect. Répéter jusqu’à **cinq
   échecs**, en attendant **au moins deux secondes entre les soumissions**.
4. Après le cinquième échec, attendre encore deux secondes, puis essayer le bon
   mot de passe : l’accès reste refusé pendant le blocage temporaire.
   Le message reste volontairement générique, par exemple **identifiant ou mot
   de passe incorrect**. Il ne révèle pas à un attaquant que le compte est bloqué.
5. Attendre **65 secondes à partir du cinquième échec**, puis réessayer le bon
   mot de passe : la connexion fonctionne à nouveau, avec l’OTP habituel si le
   compte en possède un. La réussite réinitialise le compteur.

Des soumissions espacées de moins d’une seconde peuvent déclencher la protection
« Quick Login » **avant le cinquième échec**. C’est normal, mais cela brouille
le scénario : respecter les deux secondes d’intervalle.

Attendre la fin du délai n’efface pas à lui seul le compteur. Si le mot de passe
est encore incorrect après le déblocage, un nouveau blocage peut donc survenir.
Une authentification réussie, ou 15 minutes sans nouvel échec comptabilisé,
remet le compteur à zéro. Les tentatives pendant le blocage ne l’incrémentent pas.

## Ce que cette protection démontre — et ses limites

- Keycloak refuse de nouvelles authentifications par mot de passe pour le
  compte concerné, même avec le bon mot de passe pendant le délai.
- Le blocage est temporaire, pas une désactivation permanente du compte.
- Le mécanisme est lié au compte, pas uniquement à l’adresse IP.
- Les sessions et jetons déjà délivrés ne sont pas révoqués par ce blocage.
- Ce n’est ni une limitation des appels à l’API Rust, ni une protection complète
  contre une attaque distribuée ou un déni de service. Un tiers peut aussi
  provoquer le blocage temporaire d’un compte connu.

Les durées courtes sont choisies pour la démonstration, pas comme politique de
production universelle. Aucun script d’attaque ni écran de sécurité personnalisé
n’est nécessaire.

Référence : [protection brute force — Keycloak 26.8.1](https://www.keycloak.org/docs/26.8.1/server_admin/index.html#password-guess-brute-force-attacks).
