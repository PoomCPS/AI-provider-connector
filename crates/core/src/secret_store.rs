//! SecretStore trait definition and implementations (InMemory and Keyring).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::types::{ApiKey, ApiKeyError, ProviderKind};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SecretStoreError {
    #[error("key not found for provider {0}")]
    NotFound(ProviderKind),
    #[error("keychain is unavailable: {0}")]
    Unavailable(String),
    #[error("invalid API key: {0}")]
    InvalidKey(#[from] ApiKeyError),
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

/// OS-native keychain adapter backed by the `keyring` crate.
/// Persists keys in macOS Keychain, Windows Credential Manager, or Linux Secret Service.
pub struct KeyringSecretStore {
    service_name: String,
}

impl Default for KeyringSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyringSecretStore {
    pub fn new() -> Self {
        Self {
            service_name: "ai-provider-connector".to_string(),
        }
    }

    pub fn with_service_name(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
        }
    }

    fn entry(&self, provider: ProviderKind) -> Result<keyring::Entry, SecretStoreError> {
        keyring::Entry::new(&self.service_name, provider.as_str())
            .map_err(|e| SecretStoreError::Unavailable(e.to_string()))
    }
}

impl SecretStore for KeyringSecretStore {
    fn get(&self, provider: ProviderKind) -> Result<Option<ApiKey>, SecretStoreError> {
        let entry = self.entry(provider)?;
        match entry.get_password() {
            Ok(secret) => {
                let key = ApiKey::new(&secret).map_err(SecretStoreError::InvalidKey)?;
                Ok(Some(key))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(SecretStoreError::Unavailable(e.to_string())),
        }
    }

    fn set(&self, provider: ProviderKind, key: ApiKey) -> Result<(), SecretStoreError> {
        // Never write an unvalidated string
        let _ = ApiKey::new(key.expose_secret()).map_err(SecretStoreError::InvalidKey)?;
        let entry = self.entry(provider)?;
        entry
            .set_password(key.expose_secret())
            .map_err(|e| SecretStoreError::Unavailable(e.to_string()))?;
        Ok(())
    }

    fn delete(&self, provider: ProviderKind) -> Result<(), SecretStoreError> {
        let entry = self.entry(provider)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(SecretStoreError::Unavailable(e.to_string())),
        }
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

        let retrieved = store
            .get(ProviderKind::OpenAICompatible)
            .unwrap()
            .expect("key should exist");
        assert_eq!(retrieved.expose_secret(), "sk-test-12345678");

        store.delete(ProviderKind::OpenAICompatible).unwrap();
        assert_eq!(store.get(ProviderKind::OpenAICompatible).unwrap(), None);
    }

    #[test]
    fn test_keyring_secret_store_lifecycle() {
        let store = KeyringSecretStore::with_service_name("ai-provider-connector-unit-test");
        let provider = ProviderKind::OpenAICompatible;

        // Ensure clean initial state
        let _ = store.delete(provider);

        // Verify get on empty entry
        match store.get(provider) {
            Ok(None) => {}
            Err(SecretStoreError::Unavailable(e)) => {
                eprintln!("Keychain unavailable on host, skipping test: {e}");
                return;
            }
            other => panic!("expected Ok(None), got {:?}", other),
        }

        // Set key
        let key = ApiKey::new("sk-test-keyring-lifecycle").unwrap();
        if let Err(SecretStoreError::Unavailable(e)) = store.set(provider, key.clone()) {
            eprintln!("Keychain unavailable on host, skipping test: {e}");
            return;
        }

        // Read key back
        let retrieved = store.get(provider).unwrap().expect("key should exist in keychain");
        assert_eq!(retrieved.expose_secret(), "sk-test-keyring-lifecycle");

        // Delete key
        store.delete(provider).unwrap();

        // Verify key is gone
        assert_eq!(store.get(provider).unwrap(), None);
    }
}
