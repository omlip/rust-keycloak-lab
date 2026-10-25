use crate::alerts::{BudgetRule, BudgetRules, Recipient};
pub use crate::catalog::Selection;
use crate::integrations::CatalogConnection;
use crate::sessions::Session;

pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn money(cents: u64) -> String {
    format!("{},{:02} €", cents / 100, cents % 100)
}

pub fn page(body: &str) -> String {
    layout(body, "")
}

fn layout(body: &str, menu: &str) -> String {
    let styles = include_str!("../resources/styles.css");
    format!(
        r#"<!doctype html><html lang="fr"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="light"><link rel="icon" type="image/svg+xml" href="/logo.svg"><title>Cost Monitor — Olaninc</title><style>{styles}</style><script src="/dialogs.js" defer></script></head><body><main><header class="topbar"><div class="brand"><img src="/logo.svg" alt=""><div><h1>Cost Monitor</h1><p>Suivi des coûts</p></div></div>{menu}</header>{body}</main></body></html>"#
    )
}

pub fn welcome() -> String {
    page(
        r#"<div class="welcome-main"><section class="hero" aria-labelledby="welcome-title"><div class="welcome-icon" aria-hidden="true"><img src="/logo.svg" width="64" height="64" alt=""></div><h2 id="welcome-title">Cost Monitor</h2><p class="lead">Connecte-toi avec Keycloak pour suivre le coût de tes sélections de services.</p><a class="button" href="/login">Se connecter <span aria-hidden="true">→</span></a><p class="hint">Tu seras redirigé vers la page de connexion.</p></section></div>"#,
    )
}

pub fn dashboard(
    session: &Session,
    catalog: Option<&CatalogConnection>,
    selections: &[Selection],
    message: Option<&str>,
    budgets: &BudgetRules,
    allow_unverified_email: bool,
) -> String {
    let mut body = String::new();
    if let Some(message) = message {
        body.push_str(&format!(
            "<p class=\"error\" role=\"alert\">{}</p>",
            escape(message)
        ));
    }
    let csrf = escape(&session.csrf);
    let connected = catalog.is_some_and(|c| c.tokens.is_some());
    if !connected {
        body.push_str("<section class=\"catalog-connect\" aria-labelledby=\"catalog-connect-title\"><h2 id=\"catalog-connect-title\">Suivre mes sélections</h2>");
        body.push_str(if catalog.is_some() {
            "<p>Reconnecte ton catalogue pour retrouver tes sélections.</p>"
        } else {
            "<p>Connecte ton catalogue pour suivre le coût de tes sélections.</p>"
        });
        let label = if catalog.is_some() {
            "Reconnecter mon catalogue"
        } else {
            "Connecter mon catalogue"
        };
        body.push_str(&format!(
            "<div class=\"connect-form\">{}</div>",
            action("/catalog/connect", label, &csrf)
        ));
        body.push_str("</section>");
    }
    if connected && message.is_none() {
        body.push_str("<div class=\"section-toolbar\"><h2>Mes sélections</h2><a class=\"button secondary\" href=\"/\">Actualiser les coûts</a></div>");
        if selections.is_empty() {
            body.push_str("<section>Aucune sélection. Crée tes sélections dans l’application catalogue, puis actualise cette page.</section>");
        }
        body.push_str("<div class=\"grid\">");
        for item in selections {
            let rule = budgets.rules.get(&item.id);
            let label = if rule.is_some() {
                format!("Modifier l’alerte de {}", item.name)
            } else {
                format!("Configurer une alerte pour {}", item.name)
            };
            let active = if rule.is_some() {
                " bell-configured"
            } else {
                ""
            };
            body.push_str(&format!(r#"<article><div class="selection-card-heading"><h3>{}</h3><button type="button" class="bell-button{active}" data-alert-dialog="alert-dialog-{}" aria-controls="alert-dialog-{}" aria-haspopup="dialog" aria-label="{}" title="{}"><svg aria-hidden="true" width="24" height="24" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9Z"/><path class="bell-clapper" d="M10 21h4"/></svg></button></div><p class="hint">{} service(s)</p><strong class="price">{} / mois</strong>"#,
                escape(&item.name), item.id, item.id, escape(&label), escape(&label), item.services.len(), money(item.monthly_total_cents)));
            if let Some(rule) = rule {
                body.push_str(&format!(
                    "<p class=\"alert-threshold\">Seuil d’alerte : {} / mois</p>",
                    money(rule.threshold_cents)
                ));
                if item.monthly_total_cents > rule.threshold_cents {
                    body.push_str("<p class=\"budget-exceeded\">Dépassement du seuil</p>");
                }
            }
            body.push_str("<ul>");
            for service in &item.services {
                body.push_str(&format!(
                    "<li>{} — {} / mois</li>",
                    escape(&service.name),
                    money(service.monthly_price_cents)
                ));
            }
            body.push_str("</ul></article>");
            let recipient = catalog
                .and_then(|c| c.tokens.as_ref())
                .and_then(|t| t.recipient.as_ref());
            body.push_str(&alert_dialog(
                item,
                rule,
                recipient,
                allow_unverified_email,
                &csrf,
            ));
        }
        body.push_str("</div>");
        let total = selections
            .iter()
            .try_fold(0u64, |sum, s| sum.checked_add(s.monthly_total_cents));
        body.push_str(&format!(
            "<section class=\"total\"><h2>Total mensuel des sélections</h2><strong>{}</strong><p class=\"hint\">Un service présent dans plusieurs sélections est compté dans chacune.</p></section>",
            total
                .map(money)
                .unwrap_or_else(|| "Montant total non représentable".into())
        ));
    }
    layout(&body, &account_menu(session, catalog, &csrf))
}

fn alert_dialog(
    selection: &Selection,
    rule: Option<&BudgetRule>,
    recipient: Option<&Recipient>,
    allow_unverified: bool,
    csrf: &str,
) -> String {
    let id = selection.id;
    let value = rule
        .map(|r| format!("{}.{:02}", r.threshold_cents / 100, r.threshold_cents % 100))
        .unwrap_or_default();
    let ready = recipient.is_some_and(|r| {
        (r.verified || allow_unverified) && r.address.parse::<lettre::message::Mailbox>().is_ok()
    });
    let destination = if ready {
        format!(
            "<p class=\"hint\">Notifications envoyées à {} : dépassement puis retour au seuil ou en dessous, sans rappels. Contrôle chaque minute.</p>",
            escape(&recipient.unwrap().address)
        )
    } else {
        "<p class=\"error\">Une adresse e-mail autorisée est nécessaire. Vérifie ton profil Keycloak puis reconnecte le catalogue.</p>".to_owned()
    };
    let disabled = if ready { "" } else { " disabled" };
    let remove = if rule.is_some() {
        format!(
            r#"<form method="post" action="/alerts/{id}/delete" class="alert-delete"><input type="hidden" name="csrf" value="{csrf}"><button class="secondary danger">Supprimer l’alerte</button></form>"#
        )
    } else {
        String::new()
    };
    format!(
        r#"<dialog class="alert-dialog" id="alert-dialog-{id}" aria-labelledby="alert-title-{id}"><div class="dialog-body"><div class="dialog-heading"><h2 id="alert-title-{id}">Alerte — {name}</h2><button type="button" class="secondary dialog-close" data-close-dialog aria-label="Fermer la configuration d’alerte">×</button></div><p>Coût mensuel actuel : <strong>{total}</strong></p><form method="post" action="/alerts/{id}" data-alert-form><input type="hidden" name="csrf" value="{csrf}"><label for="alert-value-{id}">Seuil mensuel (€)</label><input id="alert-value-{id}" name="threshold" type="text" inputmode="decimal" pattern="[0-9]+([.,][0-9]{{1,2}})?" value="{value}" placeholder="250,00" required autofocus>{destination}<div class="dialog-actions"><button type="submit"{disabled}>Enregistrer</button><button type="button" class="secondary" data-close-dialog>Annuler</button></div></form>{remove}</div></dialog>"#,
        name = escape(&selection.name),
        total = money(selection.monthly_total_cents)
    )
}

fn initials(name: &str) -> String {
    let words: Vec<Vec<char>> = name
        .split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<Vec<_>>()
        })
        .filter(|word| !word.is_empty())
        .collect();
    let letters: Vec<char> = match words.as_slice() {
        [] => vec![],
        [word] => word.iter().copied().take(2).collect(),
        _ => vec![words[0][0], words[words.len() - 1][0]],
    };
    let result: String = letters
        .into_iter()
        .flat_map(char::to_uppercase)
        .take(2)
        .collect();
    if result.is_empty() {
        "?".into()
    } else {
        result
    }
}

fn account_menu(session: &Session, catalog: Option<&CatalogConnection>, csrf: &str) -> String {
    let name = escape(&session.identity.name);
    let avatar = escape(&initials(&session.identity.name));
    let mut actions = String::new();
    if catalog.is_some_and(|c| c.tokens.is_some()) {
        actions.push_str(&action("/catalog/refresh", "Renouveler maintenant", csrf));
    }
    if catalog.is_some() {
        actions.push_str(&action(
            "/catalog/disconnect",
            "Déconnecter le catalogue",
            csrf,
        ));
    }
    actions.push_str(&format!(
        "<div class=\"logout-action\">{}</div>",
        action("/logout", "Se déconnecter de Cost Monitor", csrf)
    ));
    // Declarative popover: native keyboard activation, light dismiss and Escape,
    // without introducing JavaScript or relaxing the script-free CSP.
    format!(
        r#"<div class="account-area"><div class="account-identity" role="group" aria-label="Connecté en tant que {name}" title="{name}"><span class="account-avatar" aria-hidden="true">{avatar}</span><span class="identity-name" aria-hidden="true">{name}</span></div><div class="account-menu"><button type="button" class="secondary account-menu-toggle" popovertarget="account-menu-panel" aria-controls="account-menu-panel" aria-label="Menu du compte"><svg aria-hidden="true" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 6h16M4 12h16M4 18h16"/></svg></button><div id="account-menu-panel" class="account-menu-panel" popover="auto"><div class="menu-identity"><p>Connecté en tant que</p><strong>{name}</strong></div><nav aria-label="Actions du compte">{actions}</nav></div></div></div>"#
    )
}

fn action(path: &str, label: &str, csrf: &str) -> String {
    format!(
        r#"<form method="post" action="{path}"><input type="hidden" name="csrf" value="{csrf}"><button>{label}</button></form>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn welcome_matches_catalog_hero_and_preserves_the_login_route() {
        let html = welcome();
        for expected in [
            "class=\"welcome-main\"",
            "class=\"hero\" aria-labelledby=\"welcome-title\"",
            "id=\"welcome-title\">Cost Monitor",
            "class=\"welcome-icon\" aria-hidden=\"true\"",
            "width=\"64\" height=\"64\"",
            "class=\"lead\"",
            "href=\"/login\">Se connecter",
            "aria-hidden=\"true\">→",
            "Tu seras redirigé vers la page de connexion.",
        ] {
            assert!(html.contains(expected), "Missing {expected}");
        }
        assert!(!html.contains("class=\"greeting\""));
        assert!(!html.contains("Connecter mon catalogue"));
    }
    #[test]
    fn html_is_escaped_and_money_uses_integer_cents() {
        assert_eq!(escape("<script>'\"&"), "&lt;script&gt;&#39;&quot;&amp;");
        assert_eq!(money(1750), "17,50 €");
        assert_eq!(money(0), "0,00 €");
    }

    #[test]
    fn alert_modal_escapes_names_and_requires_an_authorized_recipient() {
        let selection = Selection {
            id: 7,
            name: "<Production>".into(),
            monthly_total_cents: 2500,
            services: vec![],
        };
        let rule = BudgetRule::new(2000);
        let mut recipient = Recipient {
            address: "alice@example.test".into(),
            verified: false,
        };
        let blocked = alert_dialog(&selection, Some(&rule), Some(&recipient), false, "csrf");
        assert!(blocked.contains("&lt;Production&gt;") && !blocked.contains("<Production>"));
        assert!(blocked.contains("type=\"submit\" disabled"));
        assert!(blocked.contains("action=\"/alerts/7/delete\""));
        assert!(blocked.contains("value=\"20.00\""));
        recipient.verified = true;
        let ready = alert_dialog(&selection, Some(&rule), Some(&recipient), false, "csrf");
        assert!(!ready.contains(" disabled"));
        assert!(ready.contains("alice@example.test"));
        assert!(ready.contains("aria-labelledby=\"alert-title-7\""));
        let absent = alert_dialog(&selection, None, None, true, "csrf");
        assert!(absent.contains("type=\"submit\" disabled"));
        assert!(!absent.contains("/delete"));
    }

    #[test]
    fn dashboard_is_concise_and_account_actions_live_in_the_header_popover() {
        use crate::oidc::Identity;
        let session = Session::new(Identity {
            issuer: "https://id.test".into(),
            subject: "private-subject".into(),
            name: "<Alice>".into(),
        });
        let disconnected = dashboard(&session, None, &[], None, &BudgetRules::default(), false);
        assert!(disconnected.contains("aria-label=\"Connecté en tant que &lt;Alice&gt;\""));
        assert!(!disconnected.contains("Bonjour"));
        assert!(disconnected.contains("connect-form"));
        assert!(disconnected.contains("Connecter mon catalogue"));
        assert!(!disconnected.contains("private-subject"));
        assert!(!disconnected.contains("Connexion au catalogue"));
        assert!(!disconnected.contains("Renouvellements réussis"));
        assert!(!disconnected.contains("Access token"));
        assert!(disconnected.contains("popover=\"auto\""));
        assert!(
            disconnected.find("action=\"/logout\"").unwrap()
                < disconnected.find("</header>").unwrap()
        );
    }

    #[test]
    fn initials_are_short_unicode_aware_and_never_contain_markup() {
        assert_eq!(initials("Élodie Dupont"), "ÉD");
        assert_eq!(initials("alice"), "AL");
        assert_eq!(initials("  user1  "), "US");
        assert_eq!(initials("<Alice>"), "AL");
        assert_eq!(initials("<>"), "?");
    }

    #[tokio::test]
    async fn menu_actions_follow_the_connection_state_and_costs_remain_visible() {
        use crate::{
            integrations::Integrations,
            oidc::{Identity, Tokens},
        };
        use openidconnect::{AccessToken, Nonce, RefreshToken};
        use std::time::Duration;
        let identity = Identity {
            issuer: "https://id.test".into(),
            subject: "private-subject".into(),
            name: "Alice".into(),
        };
        let store = Integrations::default();
        let slot = store.get_or_create(&identity).await.unwrap();
        let mut integration = slot.lock().await;
        let epoch = integration.begin_catalog();
        integration
            .attach_catalog(
                &identity,
                Tokens {
                    recipient: None,
                    access: AccessToken::new("private-access".into()),
                    offline: RefreshToken::new("private-offline".into()),
                    lifetime: Duration::from_secs(60),
                },
                Nonce::new_random(),
                epoch,
            )
            .unwrap();
        let session = Session::new(identity);
        let selections = [Selection {
            id: 1,
            name: "Production".into(),
            monthly_total_cents: 1250,
            services: vec![],
        }];
        let connected = dashboard(
            &session,
            integration.catalog.as_ref(),
            &selections,
            None,
            &integration.budgets,
            false,
        );
        let header_end = connected.find("</header>").unwrap();
        assert!(connected.find("action=\"/catalog/refresh\"").unwrap() < header_end);
        assert!(connected.find("action=\"/catalog/disconnect\"").unwrap() < header_end);
        assert!(connected.contains("Production") && connected.contains("12,50 €"));
        assert!(!connected.contains("Connecter mon catalogue"));
        assert!(!connected.contains("private-access") && !connected.contains("private-offline"));
        assert!(connected.contains("Configurer une alerte pour Production"));
        assert!(!connected.contains("bell-button bell-configured"));
        integration.budgets.configure(1, 1000).unwrap();
        let configured = dashboard(
            &session,
            integration.catalog.as_ref(),
            &selections,
            None,
            &integration.budgets,
            false,
        );
        assert!(configured.contains("bell-button bell-configured"));
        assert!(configured.contains("Modifier l’alerte de Production"));
        assert!(configured.contains("Seuil d’alerte : 10,00 € / mois"));
        assert!(configured.contains("Dépassement du seuil"));
        integration.catalog.as_mut().unwrap().tokens = None;
        let revoked = dashboard(
            &session,
            integration.catalog.as_ref(),
            &[],
            None,
            &integration.budgets,
            false,
        );
        assert!(revoked.contains("Reconnecter mon catalogue"));
        assert!(revoked.contains("Déconnecter le catalogue"));
        assert!(!revoked.contains("Renouveler maintenant"));
    }
}
