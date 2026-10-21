use olaninc_api::{
    app_with_catalog,
    auth::{AuthConfig, Authenticator},
    catalog::{catalog_service::CatalogService, in_memory_repository::InMemoryCatalogRepository},
};
use std::{env, error::Error, sync::Arc};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let auth = Authenticator::new(AuthConfig {
        issuer: env::var("OIDC_ISSUER")?,
        audience: env::var("OIDC_AUDIENCE")?,
        jwks_url: env::var("OIDC_JWKS_URL")?,
    })?;
    // Fail startup rather than report a ready API without a usable Keycloak key set.
    auth.warm_up().await?;
    let address = env::var("BIND_ADDRESS").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    tracing::info!(%address, "API prête");
    let catalog = CatalogService::new(Arc::new(InMemoryCatalogRepository::default()));
    axum::serve(listener, app_with_catalog(auth, catalog))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("installation du signal SIGTERM");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
