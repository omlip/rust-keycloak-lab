use crate::domain::catalog::{Category, ServiceDraft};
use serde::Deserialize;

#[derive(Deserialize)]
struct SeedEntry {
    name: String,
    category: String,
    description: String,
    monthly_price_cents: u64,
}

pub(crate) fn load() -> Result<Vec<ServiceDraft>, String> {
    let entries: Vec<SeedEntry> =
        serde_json::from_str(include_str!("../../../data/demo_catalog.json"))
            .map_err(|error| error.to_string())?;
    entries
        .into_iter()
        .map(|entry| {
            Ok(ServiceDraft {
                name: entry.name,
                category: Category::from_code(&entry.category)
                    .ok_or_else(|| format!("Unknown seed category: {}", entry.category))?,
                description: entry.description,
                monthly_price_cents: entry.monthly_price_cents,
            })
        })
        .collect()
}
