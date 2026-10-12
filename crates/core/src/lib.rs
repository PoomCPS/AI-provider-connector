//! Core library for AI Provider Connector.
//!
//! Provides a unified interface for connecting multiple AI providers
//! (OpenAI-compatible, Anthropic, Gemini) with validated newtypes,
//! credential storage, and a single normalized facade.

pub mod adapter;
pub mod config;
pub mod connector;
pub mod protocol;
pub mod secret_store;
pub mod types;

pub use adapter::{AdapterError, ProviderAdapter};
pub use config::{ConfigError, ConnectorConfig, ConnectorConfigBuilder};
pub use connector::{Connector, ConnectorError};
pub use protocol::{
    CompletionRequest, CompletionResponse, FinishReason, Message, StreamChunk, ToolCall,
    ToolDefinition,
};
pub use secret_store::{InMemorySecretStore, SecretStore, SecretStoreError};
pub use types::{
    ApiKey, ApiKeyError, EndpointUrl, EndpointUrlError, ModelId, ModelIdError, ProviderKind,
};
