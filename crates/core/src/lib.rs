//! Core library for AI Provider Connector.
//!
//! Provides a unified interface for connecting multiple AI providers
//! (OpenAI-compatible, Anthropic, Gemini) with validated newtypes,
//! credential storage, and a single normalized facade.

pub mod config;
pub mod secret_store;
pub mod types;

pub use config::{ConfigError, ConnectorConfig, ConnectorConfigBuilder};
pub use secret_store::{SecretStore, SecretStoreError};
pub use types::{
    ApiKey, ApiKeyError, EndpointUrl, EndpointUrlError, ModelId, ModelIdError, ProviderKind,
};
