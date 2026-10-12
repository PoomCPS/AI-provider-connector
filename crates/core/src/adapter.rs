//! ProviderAdapter async trait and error types.

use std::time::Duration;
use async_trait::async_trait;
use futures::stream::BoxStream;
use crate::protocol::{CompletionRequest, CompletionResponse, StreamChunk};

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum AdapterError {
    #[error("rate limit exceeded")]
    RateLimit {
        retry_after: Option<Duration>,
    },
    #[error("service unavailable")]
    ServiceUnavailable,
    #[error("authentication failed: {0}")]
    AuthenticationFailed(String),
    #[error("authorization failed: {0}")]
    AuthorizationFailed(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("provider HTTP {status}: {message}")]
    Http {
        status: u16,
        message: String,
        retry_after: Option<Duration>,
    },
    #[error("serialization/deserialization error: {0}")]
    Serialization(String),
    #[error("network / connection error: {0}")]
    Network(String),
    #[error("unsupported feature or adapter error: {0}")]
    Other(String),
}

impl AdapterError {
    /// Returns true if this error represents a transient condition eligible for retry.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::RateLimit { .. } => true,
            Self::ServiceUnavailable => true,
            Self::Http { status, .. } => *status == 429 || *status == 503,
            Self::Network(_) => true,
            Self::AuthenticationFailed(_) | Self::AuthorizationFailed(_) | Self::InvalidRequest(_) => false,
            _ => false,
        }
    }

    /// Returns the recommended retry delay if conveyed by the provider's `Retry-After` header.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimit { retry_after } => *retry_after,
            Self::Http { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

/// The single primary seam for AI provider communication across the codebase.
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    /// Executes a single, non-streaming completion request.
    async fn complete(&self, request: &CompletionRequest) -> Result<CompletionResponse, AdapterError>;

    /// Executes a streaming completion request, returning a stream of incremental token chunks.
    async fn stream(&self, request: &CompletionRequest) -> Result<BoxStream<'static, Result<StreamChunk, AdapterError>>, AdapterError>;
}
