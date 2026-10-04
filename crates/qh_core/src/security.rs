pub mod apikey;
use crate::audit::AuditStore;
use crate::config::AuditConfig;
use crate::error::AuditError;

pub struct AuditService {
    store: AuditStore,
}

impl AuditService {
    pub async fn new(cfg: &AuditConfig) -> Result<Self, AuditError> {
        let store = AuditStore::new(cfg.enabled, &cfg.path).await?;
        Ok(Self { store })
    }

    pub fn store(&self) -> &AuditStore {
        &self.store
    }
}

