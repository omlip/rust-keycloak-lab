use crate::integrations::CatalogConnection;
use crate::sessions::Session;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Selection {
    pub name: String,
    pub monthly_total_cents: u64,
    pub services: Vec<Service>,
}
#[derive(Deserialize)]
pub struct Service {
    pub name: String,
    pub monthly_price_cents: u64,
}

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
        r#"<!doctype html><html lang="fr"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="light"><link rel="icon" type="image/svg+xml" href="/logo.svg"><title>Cost Monitor — Olaninc</title><style>{styles}</style></head><body><main><header class="topbar"><div class="brand"><img src="/logo.svg" alt=""><div><h1>Cost Monitor</h1><p>Suivi des coûts</p></div></div>{menu}</header>{body}</main></body></html>"#
    )
}

pub fn welcome() -> String {
    page(
        r#"<section class="greeting"><h2>Bienvenue dans Cost Monitor</h2><p>Retrouve tes sélections de services et suis leur coût mensuel.</p><a class="button" href="/login">Se connecter à Cost Monitor</a></section>"#,
    )
}

pub fn dashboard(
    session: &Session,
    catalog: Option<&CatalogConnection>,
    selections: &[Selection],
    message: Option<&str>,
) -> String {
    let mut body = format!(
        "<section class=\"greeting\"><h2>Bonjour {}</h2>",
        escape(&session.identity.name),
    );
    if let Some(message) = message {
        body.push_str(&format!(
            "<p class=\"error\" role=\"alert\">{}</p>",
            escape(message)
        ));
    }
    let csrf = escape(&session.csrf);
    let connected = catalog.is_some_and(|c| c.tokens.is_some());
    if !connected {
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
    }
    body.push_str("</section>");
    if connected && message.is_none() {
        body.push_str("<div class=\"section-toolbar\"><h2>Mes sélections</h2><a class=\"button secondary\" href=\"/\">Actualiser les coûts</a></div>");
        if selections.is_empty() {
            body.push_str("<section>Aucune sélection. Crée tes sélections dans l’application catalogue, puis actualise cette page.</section>");
        }
        body.push_str("<div class=\"grid\">");
        for item in selections {
            body.push_str(&format!("<article><h3>{}</h3><p class=\"hint\">{} service(s)</p><strong class=\"price\">{} / mois</strong><ul>", escape(&item.name), item.services.len(), money(item.monthly_total_cents)));
            for service in &item.services {
                body.push_str(&format!(
                    "<li>{} — {} / mois</li>",
                    escape(&service.name),
                    money(service.monthly_price_cents)
                ));
            }
            body.push_str("</ul></article>");
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
    layout(&body, &account_menu(catalog, &csrf))
}

fn account_menu(catalog: Option<&CatalogConnection>, csrf: &str) -> String {
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
        r#"<div class="account-menu"><button type="button" class="secondary account-menu-toggle" popovertarget="account-menu-panel" aria-controls="account-menu-panel" aria-label="Menu du compte"><svg aria-hidden="true" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 6h16M4 12h16M4 18h16"/></svg></button><div id="account-menu-panel" class="account-menu-panel" popover="auto"><nav aria-label="Actions du compte">{actions}</nav></div></div>"#
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
    fn html_is_escaped_and_money_uses_integer_cents() {
        assert_eq!(escape("<script>'\"&"), "&lt;script&gt;&#39;&quot;&amp;");
        assert_eq!(money(1750), "17,50 €");
        assert_eq!(money(0), "0,00 €");
    }

    #[test]
    fn dashboard_is_concise_and_account_actions_live_in_the_header_popover() {
        use crate::oidc::Identity;
        let session = Session::new(Identity {
            issuer: "https://id.test".into(),
            subject: "private-subject".into(),
            name: "<Alice>".into(),
        });
        let disconnected = dashboard(&session, None, &[], None);
        assert!(disconnected.contains("Bonjour &lt;Alice&gt;"));
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
            name: "Production".into(),
            monthly_total_cents: 1250,
            services: vec![],
        }];
        let connected = dashboard(&session, integration.catalog.as_ref(), &selections, None);
        let header_end = connected.find("</header>").unwrap();
        assert!(connected.find("action=\"/catalog/refresh\"").unwrap() < header_end);
        assert!(connected.find("action=\"/catalog/disconnect\"").unwrap() < header_end);
        assert!(connected.contains("Production") && connected.contains("12,50 €"));
        assert!(!connected.contains("Connecter mon catalogue"));
        assert!(!connected.contains("private-access") && !connected.contains("private-offline"));
        integration.catalog.as_mut().unwrap().tokens = None;
        let revoked = dashboard(&session, integration.catalog.as_ref(), &[], None);
        assert!(revoked.contains("Reconnecter mon catalogue"));
        assert!(revoked.contains("Déconnecter le catalogue"));
        assert!(!revoked.contains("Renouveler maintenant"));
    }
}
