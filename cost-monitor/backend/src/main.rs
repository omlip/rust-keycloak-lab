use cost_monitor::mail::SmtpMailer;
use cost_monitor::{
    auth::Authenticator,
    oidc::Oidc,
    web::{self, Config},
};
use std::sync::Arc;
use std::{env, error::Error, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let origin = env::var("PUBLIC_ORIGIN").unwrap_or_else(|_| "http://localhost:8082".into());
    let issuer =
        env::var("OIDC_ISSUER").unwrap_or_else(|_| "http://localhost:8081/realms/olaninc".into());
    let internal = env::var("OIDC_INTERNAL_ISSUER").unwrap_or_else(|_| issuer.clone());
    let api = env::var("API_BASE").unwrap_or_else(|_| "http://localhost:8080".into());
    for value in [&origin, &issuer, &internal, &api] {
        let url = reqwest::Url::parse(value)?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("Invalid endpoint configuration".into());
        }
    }
    let catalog_path = env::var("CATALOG_CLIENT_SECRET_FILE")
        .unwrap_or_else(|_| "../../.secrets/cost-monitor.secret".into());
    let catalog_secret = std::fs::read_to_string(catalog_path)?.trim().to_owned();
    if catalog_secret.is_empty() {
        return Err("Empty client secret".into());
    }
    // Keycloak can become ready just before its discovery endpoint responds.
    let mut client = None;
    for _ in 0..15 {
        let (auth, catalog) = tokio::join!(
            Authenticator::discover(&issuer, &internal),
            Oidc::discover(&issuer, &internal, &origin, catalog_secret.clone())
        );
        if let (Ok(auth), Ok(catalog)) = (auth, catalog) {
            client = Some((auth, catalog));
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let (auth, catalog_client) = client.ok_or("OIDC discovery failed; no credentials logged")?;
    let listener = tokio::net::TcpListener::bind(
        env::var("BIND_ADDRESS").unwrap_or_else(|_| "127.0.0.1:3002".into()),
    )
    .await?;
    println!(
        "Cost Monitor ready: SPA Bearer authentication and optional offline catalogue delegation."
    );
    let mailer = Arc::new(SmtpMailer::new(
        &env::var("SMTP_HOST").unwrap_or_else(|_| "localhost".into()),
        env::var("SMTP_PORT")
            .unwrap_or_else(|_| "1025".into())
            .parse()?,
        &env::var("SMTP_FROM")
            .unwrap_or_else(|_| "Cost Monitor <cost-monitor@olaninc.test>".into()),
        &origin,
    )?);
    let (application, monitoring) = web::app(
        auth,
        catalog_client,
        Config {
            origin,
            issuer,
            api,
            allow_unverified_email: env::var("ALLOW_UNVERIFIED_ALERT_EMAIL").as_deref()
                == Ok("true"),
        },
        mailer,
    );
    let result = axum::serve(listener, application)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    monitoring.abort();
    result?;
    Ok(())
}
