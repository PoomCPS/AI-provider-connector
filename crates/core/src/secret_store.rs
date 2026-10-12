//! SecretStore trait definition and in-memory implementation for testing.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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

/// Thread-safe in-memory SecretStore backed by a `HashMap` and `Arc<Mutex<>>`.
/// Ideal for unit/integration testing without touching the OS keychain.
#[derive(Clone, Default)]
pub struct InMemorySecretStore {
    keys: Arc<Mutex<HashMap<ProviderKind, ApiKey>>>,
}

impl InMemorySecretStore {
    pub fn new() -> Self {
        Self {
            keys: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl SecretStore for InMemorySecretStore {
    fn get(&self, provider: ProviderKind) -> Result<Option<ApiKey>, SecretStoreError> {
        let guard = self
            .keys
            .lock()
            .map_err(|e| SecretStoreError::Backend(e.to_string()))?;
        Ok(guard.get(&provider).cloned())
    }

    fn set(&self, provider: ProviderKind, key: ApiKey) -> Result<(), SecretStoreError> {
        let mut guard = self
            .keys
            .lock()
            .map_err(|e| SecretStoreError::Backend(e.to_string()))?;
        guard.insert(provider, key);
        Ok(())
    }

    fn delete(&self, provider: ProviderKind) -> Result<(), SecretStoreError> {
        let mut guard = self
            .keys
            .lock()
            .map_err(|e| SecretStoreError::Backend(e.to_string()))?;
        guard.remove(&provider);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_secret_store_lifecycle() {
        let store = InMemorySecretStore::new();
        assert_eq!(store.get(ProviderKind::OpenAICompatible).unwrap(), None);

        let key = ApiKey::new("sk-test-12345678").unwrap();
        store.set(ProviderKind::OpenAICompatible, key.clone()).unwrap();

        let retrieved = store.get(ProviderKind::OpenAICompatible).unwrap().expect("key should exist");
        assert_eq!(retrieved.expose_secret(), "sk-test-12345678");

        store.delete(ProviderKind::OpenAICompatible).unwrap();
        assert_eq!(store.get(ProviderKind::OpenAICompatible).unwrap(), None);
    }
}
