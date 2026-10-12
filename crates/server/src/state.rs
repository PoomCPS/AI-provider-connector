use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use ai_provider_connector_core::{
    Connector, ConnectorConfig, ConnectorError, ProviderAdapter, ProviderKind, SecretStore,
};

/// Shared application state for axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub secret_store: Arc<dyn SecretStore>,
    pub adapter_overrides: Arc<RwLock<HashMap<ProviderKind, Arc<dyn ProviderAdapter>>>>,
}

impl AppState {
    pub fn new(secret_store: Arc<dyn SecretStore>) -> Self {
        Self {
            secret_store,
            adapter_overrides: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Injects an adapter override (useful for tests or mock backends).
    pub fn set_adapter_override(
        &self,
        provider: ProviderKind,
        adapter: Arc<dyn ProviderAdapter>,
    ) {
        self.adapter_overrides
            .write()
            .expect("lock poisoned")
            .insert(provider, adapter);
    }

    /// Builds or retrieves a `Connector` for the requested provider and model.
    pub fn get_connector(
        &self,
        provider: ProviderKind,
        model: ai_provider_connector_core::ModelId,
    ) -> Result<Connector, ConnectorError> {
        let override_adapter = self
            .adapter_overrides
            .read()
            .expect("lock poisoned")
            .get(&provider)
            .cloned();

        let config = ConnectorConfig::builder()
            .provider(provider)
            .endpoint(provider.default_endpoint())
            .model(model)
            .secret_store(self.secret_store.clone())
            .build()
            .map_err(|e| ConnectorError::Adapter(ai_provider_connector_core::AdapterError::InvalidRequest(e.to_string())))?;

        if let Some(adapter) = override_adapter {
            Ok(Connector::from_adapter(config, adapter))
        } else {
            Connector::new(config)
        }
    }
}
