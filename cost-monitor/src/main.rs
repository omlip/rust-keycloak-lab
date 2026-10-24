use cost_monitor::{
    oidc::{Oidc, Purpose},
    web::{self, Config},
};
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
    let login_path = env::var("LOGIN_CLIENT_SECRET_FILE")
        .unwrap_or_else(|_| "../.secrets/cost-monitor-login.secret".into());
    let catalog_path = env::var("CATALOG_CLIENT_SECRET_FILE")
        .unwrap_or_else(|_| "../.secrets/cost-monitor.secret".into());
    let login_secret = std::fs::read_to_string(login_path)?.trim().to_owned();
    let catalog_secret = std::fs::read_to_string(catalog_path)?.trim().to_owned();
    if login_secret.is_empty() || catalog_secret.is_empty() {
        return Err("Empty client secret".into());
    }
    // Keycloak can become ready just before its discovery endpoint responds.
    let mut client = None;
    for _ in 0..15 {
        let (login, catalog) = tokio::join!(
            Oidc::discover(
                &issuer,
                &internal,
                &origin,
                login_secret.clone(),
                Purpose::AppLogin
            ),
            Oidc::discover(
                &issuer,
                &internal,
                &origin,
                catalog_secret.clone(),
                Purpose::Catalog
            )
        );
        if let (Ok(login), Ok(catalog)) = (login, catalog) {
            client = Some((login, catalog));
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let (login_client, catalog_client) =
        client.ok_or("OIDC discovery failed; no credentials logged")?;
    let listener = tokio::net::TcpListener::bind(
        env::var("BIND_ADDRESS").unwrap_or_else(|_| "127.0.0.1:8082".into()),
    )
    .await?;
    println!(
        "Cost Monitor ready: independent application login and optional offline catalogue delegation."
    );
    axum::serve(
        listener,
        web::app(
            login_client,
            catalog_client,
            Config {
                origin,
                issuer,
                api,
            },
        ),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}
