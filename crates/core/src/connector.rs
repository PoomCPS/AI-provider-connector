//! Connector facade providing a single entry point for model interactions.

use std::sync::Arc;
use futures::stream::BoxStream;
use futures::StreamExt;
use crate::adapter::{AdapterError, ProviderAdapter};
use crate::config::ConnectorConfig;
use crate::protocol::{CompletionRequest, CompletionResponse, StreamChunk};
use crate::secret_store::SecretStoreError;
use crate::types::ProviderKind;

#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    #[error("retries exhausted after {attempts} attempts, last error: {last_error}")]
    RetriesExhausted {
        attempts: u32,
        last_error: AdapterError,
    },
    #[error("adapter error: {0}")]
    Adapter(#[from] AdapterError),
    #[error("secret store error: {0}")]
    SecretStore(#[from] SecretStoreError),
    #[error("missing API key for provider {0}")]
    MissingApiKey(ProviderKind),
    #[error("provider not yet implemented: {0}")]
    NotImplemented(ProviderKind),
}

/// The unified entry point for interacting with AI models.
pub struct Connector {
    pub(crate) config: ConnectorConfig,
    pub(crate) adapter: Arc<dyn ProviderAdapter>,
}

impl Connector {
    /// Constructs a `Connector` from a validated `ConnectorConfig`.
    pub fn new(config: ConnectorConfig) -> Result<Self, ConnectorError> {
        let adapter: Arc<dyn ProviderAdapter> = match config.provider {
            ProviderKind::OpenAICompatible => Arc::new(crate::adapters::openai::OpenAIAdapter::new(
                config.endpoint.clone(),
                config.secret_store.clone(),
            )),
            ProviderKind::Anthropic => return Err(ConnectorError::NotImplemented(ProviderKind::Anthropic)),
            ProviderKind::Gemini => return Err(ConnectorError::NotImplemented(ProviderKind::Gemini)),
        };
        Ok(Self { config, adapter })
    }

    /// Constructs a `Connector` with an explicitly supplied `ProviderAdapter`.
    pub fn from_adapter(config: ConnectorConfig, adapter: Arc<dyn ProviderAdapter>) -> Self {
        Self { config, adapter }
    }

    /// Returns a reference to the active `ConnectorConfig`.
    pub fn config(&self) -> &ConnectorConfig {
        &self.config
    }

    /// Sends a completion request and awaits the complete response, retrying transient errors.
    pub async fn complete(&self, request: &CompletionRequest) -> Result<CompletionResponse, ConnectorError> {
        let mut attempt = 0;
        loop {
            match self.adapter.complete(request).await {
                Ok(resp) => return Ok(resp),
                Err(err) => {
                    attempt += 1;
                    if !err.is_retryable() {
                        return Err(ConnectorError::Adapter(err));
                    }
                    if attempt >= self.config.retry_policy.max_attempts {
                        return Err(ConnectorError::RetriesExhausted {
                            attempts: attempt,
                            last_error: err,
                        });
                    }
                    let delay = self.config.retry_policy.compute_delay(attempt - 1, err.retry_after());
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    /// Sends a completion request and returns an asynchronous stream of token chunks.
    /// Retries only the initial connection phase; mid-stream disconnects surface immediately.
    pub async fn stream(
        &self,
        request: &CompletionRequest,
    ) -> Result<BoxStream<'static, Result<StreamChunk, ConnectorError>>, ConnectorError> {
        let mut attempt = 0;
        loop {
            match self.adapter.stream(request).await {
                Ok(stream) => {
                    let mapped = stream.map(|res| res.map_err(ConnectorError::Adapter));
                    return Ok(Box::pin(mapped));
                }
                Err(err) => {
                    attempt += 1;
                    if !err.is_retryable() {
                        return Err(ConnectorError::Adapter(err));
                    }
                    if attempt >= self.config.retry_policy.max_attempts {
                        return Err(ConnectorError::RetriesExhausted {
                            attempts: attempt,
                            last_error: err,
                        });
                    }
                    let delay = self.config.retry_policy.compute_delay(attempt - 1, err.retry_after());
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;
    use async_trait::async_trait;
    use futures::stream;
    use crate::config::ConnectorConfigBuilder;
    use crate::protocol::{FinishReason, Message};
    use crate::retry::RetryPolicy;
    use crate::secret_store::{InMemorySecretStore, SecretStore};
    use crate::types::{ApiKey, ModelId};

    struct MockAdapter {
        attempts: AtomicU32,
        fail_with: Vec<AdapterError>,
        success_response: CompletionResponse,
    }

    #[async_trait]
    impl ProviderAdapter for MockAdapter {
        async fn complete(&self, _request: &CompletionRequest) -> Result<CompletionResponse, AdapterError> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) as usize;
            if attempt < self.fail_with.len() {
                Err(self.fail_with[attempt].clone())
            } else {
                Ok(self.success_response.clone())
            }
        }

        async fn stream(&self, _request: &CompletionRequest) -> Result<BoxStream<'static, Result<StreamChunk, AdapterError>>, AdapterError> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) as usize;
            if attempt < self.fail_with.len() {
                Err(self.fail_with[attempt].clone())
            } else {
                let chunks = vec![
                    Ok(StreamChunk::token("Hello")),
                    Ok(StreamChunk::done()),
                ];
                Ok(Box::pin(stream::iter(chunks)))
            }
        }
    }

    fn test_config(retry_policy: RetryPolicy) -> ConnectorConfig {
        let store = Arc::new(InMemorySecretStore::new());
        store
            .set(ProviderKind::OpenAICompatible, ApiKey::new("sk-test").unwrap())
            .unwrap();

        ConnectorConfigBuilder::new()
            .provider(ProviderKind::OpenAICompatible)
            .model(ModelId::new("gpt-4o").unwrap())
            .secret_store(store)
            .retry_policy(retry_policy)
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn test_retry_on_rate_limit_then_success() {
        tokio::time::pause();

        let mock = Arc::new(MockAdapter {
            attempts: AtomicU32::new(0),
            fail_with: vec![
                AdapterError::RateLimit { retry_after: None },
                AdapterError::RateLimit { retry_after: None },
                AdapterError::RateLimit { retry_after: None },
            ],
            success_response: CompletionResponse {
                content: "Success".into(),
                input_tokens: 10,
                output_tokens: 5,
                finish_reason: FinishReason::Stop,
                tool_calls: vec![],
            },
        });

        let policy = RetryPolicy::new(4, Duration::from_millis(10), Duration::from_millis(50));
        let connector = Connector::from_adapter(test_config(policy), mock.clone());

        let req = CompletionRequest::new(ModelId::new("gpt-4o").unwrap(), vec![Message::user("hi")]);
        let resp = connector.complete(&req).await.unwrap();

        assert_eq!(resp.content, "Success");
        assert_eq!(mock.attempts.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn test_auth_error_never_retried() {
        let mock = Arc::new(MockAdapter {
            attempts: AtomicU32::new(0),
            fail_with: vec![AdapterError::AuthenticationFailed("invalid key".into())],
            success_response: CompletionResponse {
                content: "Should not reach".into(),
                input_tokens: 0,
                output_tokens: 0,
                finish_reason: FinishReason::Stop,
                tool_calls: vec![],
            },
        });

        let policy = RetryPolicy::new(5, Duration::from_millis(10), Duration::from_millis(50));
        let connector = Connector::from_adapter(test_config(policy), mock.clone());

        let req = CompletionRequest::new(ModelId::new("gpt-4o").unwrap(), vec![Message::user("hi")]);
        let res = connector.complete(&req).await;

        assert!(matches!(res, Err(ConnectorError::Adapter(AdapterError::AuthenticationFailed(_)))));
        assert_eq!(mock.attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_after_header_honored() {
        tokio::time::pause();

        let mock = Arc::new(MockAdapter {
            attempts: AtomicU32::new(0),
            fail_with: vec![
                AdapterError::RateLimit {
                    retry_after: Some(Duration::from_millis(1)),
                },
            ],
            success_response: CompletionResponse {
                content: "Honored".into(),
                input_tokens: 1,
                output_tokens: 1,
                finish_reason: FinishReason::Stop,
                tool_calls: vec![],
            },
        });

        let policy = RetryPolicy::new(2, Duration::from_secs(10), Duration::from_secs(20));
        let connector = Connector::from_adapter(test_config(policy), mock.clone());

        let req = CompletionRequest::new(ModelId::new("gpt-4o").unwrap(), vec![Message::user("hi")]);
        let resp = connector.complete(&req).await.unwrap();

        assert_eq!(resp.content, "Honored");
        assert_eq!(mock.attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_retries_exhausted_returns_error() {
        tokio::time::pause();

        let mock = Arc::new(MockAdapter {
            attempts: AtomicU32::new(0),
            fail_with: vec![
                AdapterError::ServiceUnavailable,
                AdapterError::ServiceUnavailable,
            ],
            success_response: CompletionResponse {
                content: "Never".into(),
                input_tokens: 0,
                output_tokens: 0,
                finish_reason: FinishReason::Stop,
                tool_calls: vec![],
            },
        });

        let policy = RetryPolicy::new(2, Duration::from_millis(1), Duration::from_millis(10));
        let connector = Connector::from_adapter(test_config(policy), mock.clone());

        let req = CompletionRequest::new(ModelId::new("gpt-4o").unwrap(), vec![Message::user("hi")]);
        let res = connector.complete(&req).await;

        match res {
            Err(ConnectorError::RetriesExhausted { attempts, last_error }) => {
                assert_eq!(attempts, 2);
                assert_eq!(last_error, AdapterError::ServiceUnavailable);
            }
            other => panic!("expected RetriesExhausted, got {:?}", other.err()),
        }
    }
}
