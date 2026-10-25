use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub struct Selection {
    pub id: u64,
    pub name: String,
    pub monthly_total_cents: u64,
    pub services: Vec<Service>,
}
#[derive(Clone, Deserialize)]
pub struct Service {
    pub name: String,
    pub monthly_price_cents: u64,
}

pub struct Snapshot {
    pub instance: Option<String>,
    pub selections: Vec<Selection>,
}

pub async fn read(
    client: &reqwest::Client,
    base: &str,
    access: &str,
) -> Result<Snapshot, &'static str> {
    let response = client
        .get(format!("{base}/api/selections"))
        .bearer_auth(access)
        .send()
        .await
        .map_err(|_| "Le catalogue est temporairement indisponible.")?;
    if response.status() == reqwest::StatusCode::FORBIDDEN {
        return Err("L’accès à tes sélections n’est plus autorisé. Reconnecte ton catalogue.");
    }
    if !response.status().is_success() {
        return Err("Le catalogue a refusé la consultation. Réessaie ou reconnecte ton compte.");
    }
    let instance = response
        .headers()
        .get("x-catalog-instance")
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);
    let selections = response
        .json()
        .await
        .map_err(|_| "Réponse du catalogue incompatible.")?;
    Ok(Snapshot {
        instance,
        selections,
    })
}
