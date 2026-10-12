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
    ///
    /// In Ticket 03, concrete HTTP adapters are not yet wired, so this
    /// method returns `ConnectorError::NotImplemented`. Use `from_adapter`
    /// for custom or testing adapters.
    pub fn new(config: ConnectorConfig) -> Result<Self, ConnectorError> {
        Err(ConnectorError::NotImplemented(config.provider))
    }

    /// Constructs a `Connector` with an explicitly supplied `ProviderAdapter`.
    pub fn from_adapter(config: ConnectorConfig, adapter: Arc<dyn ProviderAdapter>) -> Self {
        Self { config, adapter }
    }

    /// Returns a reference to the active `ConnectorConfig`.
    pub fn config(&self) -> &ConnectorConfig {
        &self.config
    }

    /// Sends a completion request and awaits the complete response.
    pub async fn complete(&self, request: &CompletionRequest) -> Result<CompletionResponse, ConnectorError> {
        self.adapter.complete(request).await.map_err(ConnectorError::Adapter)
    }

    /// Sends a completion request and returns an asynchronous stream of token chunks.
    pub async fn stream(
        &self,
        request: &CompletionRequest,
    ) -> Result<BoxStream<'static, Result<StreamChunk, ConnectorError>>, ConnectorError> {
        let stream = self.adapter.stream(request).await.map_err(ConnectorError::Adapter)?;
        let mapped = stream.map(|res| res.map_err(ConnectorError::Adapter));
        Ok(Box::pin(mapped))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream;
    use crate::config::ConnectorConfigBuilder;
    use crate::protocol::{FinishReason, Message};
    use crate::secret_store::{InMemorySecretStore, SecretStore};
    use crate::types::{ApiKey, ModelId};

    struct FakeAdapter {
        canned_response: CompletionResponse,
        canned_chunks: Vec<String>,
    }

    #[async_trait]
    impl ProviderAdapter for FakeAdapter {
        async fn complete(&self, _request: &CompletionRequest) -> Result<CompletionResponse, AdapterError> {
            Ok(self.canned_response.clone())
        }

        async fn stream(&self, _request: &CompletionRequest) -> Result<BoxStream<'static, Result<StreamChunk, AdapterError>>, AdapterError> {
            let chunks: Vec<Result<StreamChunk, AdapterError>> = self
                .canned_chunks
                .iter()
                .map(|s| Ok(StreamChunk::token(s.clone())))
                .chain(std::iter::once(Ok(StreamChunk::done())))
                .collect();
            Ok(Box::pin(stream::iter(chunks)))
        }
    }

    #[tokio::test]
    async fn test_connector_complete_with_fake_adapter() {
        let secret_store = Arc::new(InMemorySecretStore::new());
        secret_store
            .set(ProviderKind::OpenAICompatible, ApiKey::new("sk-test").unwrap())
            .unwrap();

        let config = ConnectorConfigBuilder::new()
            .provider(ProviderKind::OpenAICompatible)
            .model(ModelId::new("gpt-4o").unwrap())
            .secret_store(secret_store)
            .build()
            .unwrap();

        let fake_adapter = Arc::new(FakeAdapter {
            canned_response: CompletionResponse {
                content: "Hello from fake adapter".into(),
                input_tokens: 10,
                output_tokens: 5,
                finish_reason: FinishReason::Stop,
                tool_calls: vec![],
            },
            canned_chunks: vec![],
        });

        let connector = Connector::from_adapter(config, fake_adapter);
        let req = CompletionRequest::new(ModelId::new("gpt-4o").unwrap(), vec![Message::user("hi")]);
        let resp = connector.complete(&req).await.unwrap();

        assert_eq!(resp.content, "Hello from fake adapter");
        assert_eq!(resp.input_tokens, 10);
        assert_eq!(resp.output_tokens, 5);
        assert_eq!(resp.finish_reason, FinishReason::Stop);
    }

    #[tokio::test]
    async fn test_connector_stream_with_fake_adapter() {
        let secret_store = Arc::new(InMemorySecretStore::new());
        let config = ConnectorConfigBuilder::new()
            .provider(ProviderKind::OpenAICompatible)
            .model(ModelId::new("gpt-4o").unwrap())
            .secret_store(secret_store)
            .build()
            .unwrap();

        let fake_adapter = Arc::new(FakeAdapter {
            canned_response: CompletionResponse {
                content: String::new(),
                input_tokens: 0,
                output_tokens: 0,
                finish_reason: FinishReason::Stop,
                tool_calls: vec![],
            },
            canned_chunks: vec!["Hello".into(), " ".into(), "World".into()],
        });

        let connector = Connector::from_adapter(config, fake_adapter);
        let req = CompletionRequest::new(ModelId::new("gpt-4o").unwrap(), vec![Message::user("hi")]);
        let mut stream = connector.stream(&req).await.unwrap();

        let mut collected = String::new();
        let mut saw_done = false;
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.unwrap();
            if chunk.done {
                saw_done = true;
            } else {
                collected.push_str(&chunk.delta);
            }
        }

        assert_eq!(collected, "Hello World");
        assert!(saw_done);
    }

    #[test]
    fn test_connector_new_not_implemented_stub() {
        let secret_store = Arc::new(InMemorySecretStore::new());
        let config = ConnectorConfigBuilder::new()
            .provider(ProviderKind::Gemini)
            .model(ModelId::new("gemini-1.5-pro").unwrap())
            .secret_store(secret_store)
            .build()
            .unwrap();

        let res = Connector::new(config);
        assert!(matches!(res, Err(ConnectorError::NotImplemented(ProviderKind::Gemini))));
    }
}
