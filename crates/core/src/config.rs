//! Connector configuration and builder.

use std::sync::Arc;
use crate::retry::RetryPolicy;
use crate::secret_store::SecretStore;
use crate::tls::TlsConfig;
use crate::types::{EndpointUrl, ModelId, ProviderKind};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("missing provider kind")]
    MissingProvider,
    #[error("missing model ID")]
    MissingModel,
    #[error("missing secret store")]
    MissingSecretStore,
    #[error("TLS configuration error: {0}")]
    Tls(#[from] crate::tls::TlsConfigError),
}

/// Validated configuration for constructing a `Connector`.
#[derive(Clone)]
pub struct ConnectorConfig {
    pub provider: ProviderKind,
    pub endpoint: EndpointUrl,
    pub model: ModelId,
    pub secret_store: Arc<dyn SecretStore>,
    pub retry_policy: RetryPolicy,
    pub tls: TlsConfig,
}

impl ConnectorConfig {
    pub fn builder() -> ConnectorConfigBuilder {
        ConnectorConfigBuilder::new()
    }
}

impl std::fmt::Debug for ConnectorConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectorConfig")
            .field("provider", &self.provider)
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("retry_policy", &self.retry_policy)
            .field("tls", &self.tls)
            .field("secret_store", &"<dyn SecretStore>")
            .finish()
    }
}

/// Builder for constructing a `ConnectorConfig`.
pub struct ConnectorConfigBuilder {
    provider: Option<ProviderKind>,
    endpoint: Option<EndpointUrl>,
    model: Option<ModelId>,
    secret_store: Option<Arc<dyn SecretStore>>,
    retry_policy: Option<RetryPolicy>,
    tls: Option<TlsConfig>,
}

impl Default for ConnectorConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectorConfigBuilder {
    pub fn new() -> Self {
        Self {
            provider: None,
            endpoint: None,
            model: None,
            secret_store: None,
            retry_policy: None,
            tls: None,
        }
    }

    pub fn provider(mut self, provider: ProviderKind) -> Self {
        self.provider = Some(provider);
        self
    }

    pub fn endpoint(mut self, endpoint: EndpointUrl) -> Self {
        self.endpoint = Some(endpoint);
        self
    }

    pub fn model(mut self, model: ModelId) -> Self {
        self.model = Some(model);
        self
    }

    pub fn secret_store(mut self, secret_store: Arc<dyn SecretStore>) -> Self {
        self.secret_store = Some(secret_store);
        self
    }

    pub fn retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = Some(retry_policy);
        self
    }

    pub fn tls(mut self, tls: TlsConfig) -> Self {
        self.tls = Some(tls);
        self
    }

    pub fn build(self) -> Result<ConnectorConfig, ConfigError> {
        let provider = self.provider.ok_or(ConfigError::MissingProvider)?;
        let endpoint = self.endpoint.unwrap_or_else(|| provider.default_endpoint());
        let model = self.model.ok_or(ConfigError::MissingModel)?;
        let secret_store = self.secret_store.ok_or(ConfigError::MissingSecretStore)?;
        let retry_policy = self.retry_policy.unwrap_or_default();
        let tls = self.tls.unwrap_or_default();

        Ok(ConnectorConfig {
            provider,
            endpoint,
            model,
            secret_store,
            retry_policy,
            tls,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ApiKey;
    use crate::secret_store::SecretStoreError;

    struct DummyStore;
    impl SecretStore for DummyStore {
        fn get(&self, _provider: ProviderKind) -> Result<Option<ApiKey>, SecretStoreError> {
            Ok(None)
        }
        fn set(&self, _provider: ProviderKind, _key: ApiKey) -> Result<(), SecretStoreError> {
            Ok(())
        }
        fn delete(&self, _provider: ProviderKind) -> Result<(), SecretStoreError> {
            Ok(())
        }
    }

    #[test]
    fn test_builder_success() {
        let store = Arc::new(DummyStore);
        let config = ConnectorConfigBuilder::new()
            .provider(ProviderKind::OpenAICompatible)
            .model(ModelId::new("gpt-4o").unwrap())
            .secret_store(store)
            .build()
            .expect("should build successfully with default endpoint");

        assert_eq!(config.provider, ProviderKind::OpenAICompatible);
        assert_eq!(config.endpoint.as_str(), "https://api.openai.com/v1");
        assert_eq!(config.model.as_str(), "gpt-4o");
        assert_eq!(config.retry_policy, RetryPolicy::default());
    }

    #[test]
    fn test_builder_missing_required_fields() {
        assert_eq!(
            ConnectorConfigBuilder::new().build().unwrap_err(),
            ConfigError::MissingProvider
        );

        assert_eq!(
            ConnectorConfigBuilder::new()
                .provider(ProviderKind::Anthropic)
                .build()
                .unwrap_err(),
            ConfigError::MissingModel
        );

        assert_eq!(
            ConnectorConfigBuilder::new()
                .provider(ProviderKind::Anthropic)
                .model(ModelId::new("claude-3-5-sonnet").unwrap())
                .build()
                .unwrap_err(),
            ConfigError::MissingSecretStore
        );
    }
}
