#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Compute,
    Storage,
    Backup,
    Hosting,
    Network,
}

impl Category {
    pub fn code(self) -> &'static str {
        match self {
            Self::Compute => "compute",
            Self::Storage => "storage",
            Self::Backup => "backup",
            Self::Hosting => "hosting",
            Self::Network => "network",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "compute" => Some(Self::Compute),
            "storage" => Some(Self::Storage),
            "backup" => Some(Self::Backup),
            "hosting" => Some(Self::Hosting),
            "network" => Some(Self::Network),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceDraft {
    pub name: String,
    pub category: Category,
    pub description: String,
    pub monthly_price_cents: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatacenterService {
    pub id: u64,
    pub data: ServiceDraft,
}
