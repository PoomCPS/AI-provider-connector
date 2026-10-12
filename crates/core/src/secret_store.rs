//! SecretStore trait definition for secure credential storage.

use crate::types::{ApiKey, ProviderKind};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SecretStoreError {
    #[error("key not found for provider {0}")]
    NotFound(ProviderKind),
    #[error("secret store backend error: {0}")]
    Backend(String),
}

/// Abstract seam for storing and retrieving provider API keys.
pub trait SecretStore: Send + Sync {
    /// Retrieves the stored API key for a provider, or None if not set.
    fn get(&self, provider: ProviderKind) -> Result<Option<ApiKey>, SecretStoreError>;

    /// Persists or updates the API key for a provider.
    fn set(&self, provider: ProviderKind, key: ApiKey) -> Result<(), SecretStoreError>;

    /// Deletes the API key for a provider.
    fn delete(&self, provider: ProviderKind) -> Result<(), SecretStoreError>;
}
