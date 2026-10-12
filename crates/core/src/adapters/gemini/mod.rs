//! Google Gemini REST API adapter implementation.

pub mod wire;

use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::stream::BoxStream;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use crate::adapter::{AdapterError, ProviderAdapter};
use crate::protocol::{CompletionRequest, CompletionResponse, StreamChunk};
use crate::secret_store::SecretStore;
use crate::types::{ApiKey, EndpointUrl, ProviderKind};
use wire::{
    map_gemini_http_error, parse_gemini_stream_chunk, GeminiGenerateContentRequest,
    GeminiGenerateContentResponse,
};

/// Adapter communicating with Google Gemini REST API (gemini-* models).
pub struct GeminiAdapter {
    client: reqwest::Client,
    endpoint: EndpointUrl,
    secret_store: Arc<dyn SecretStore>,
}

impl GeminiAdapter {
    pub fn new(endpoint: EndpointUrl, secret_store: Arc<dyn SecretStore>) -> Self {
        Self {
            client: reqwest::Client::new(),
            endpoint,
            secret_store,
        }
    }

    pub fn with_client(
        client: reqwest::Client,
        endpoint: EndpointUrl,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            client,
            endpoint,
            secret_store,
        }
    }

    fn get_api_key(&self) -> Result<ApiKey, AdapterError> {
        self.secret_store
            .get(ProviderKind::Gemini)
            .map_err(|e| AdapterError::AuthenticationFailed(format!("failed to fetch key from store: {e}")))?
            .ok_or_else(|| AdapterError::AuthenticationFailed("no API key set for Gemini".into()))
    }

    fn build_url(&self, model: &str, stream: bool, api_key: &ApiKey) -> String {
        let base = self.endpoint.as_str().trim_end_matches('/');
        let action = if stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        };

        let path = if base.ends_with("/v1beta") {
            format!("{}/models/{}:{}", base, model, action)
        } else {
            format!("{}/v1beta/models/{}:{}", base, model, action)
        };

        let stream_param = if stream { "&alt=sse" } else { "" };
        format!("{}?key={}{}", path, api_key.expose_secret(), stream_param)
    }

    fn extract_retry_after(headers: &HeaderMap) -> Option<Duration> {
        headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs)
    }
}

#[async_trait]
impl ProviderAdapter for GeminiAdapter {
    async fn complete(&self, request: &CompletionRequest) -> Result<CompletionResponse, AdapterError> {
        let api_key = self.get_api_key()?;
        let url = self.build_url(request.model.as_str(), false, &api_key);
        let payload = GeminiGenerateContentRequest::from_normalized(request);

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;

        let status = response.status().as_u16();
        let retry_after = Self::extract_retry_after(response.headers());

        if status != 200 {
            let body = response.text().await.unwrap_or_default();
            return Err(map_gemini_http_error(status, &body, retry_after));
        }

        let body: GeminiGenerateContentResponse = response
            .json()
            .await
            .map_err(|e| AdapterError::Serialization(e.to_string()))?;

        body.into_normalized()
    }

    async fn stream(
        &self,
        request: &CompletionRequest,
    ) -> Result<BoxStream<'static, Result<StreamChunk, AdapterError>>, AdapterError> {
        let api_key = self.get_api_key()?;
        let url = self.build_url(request.model.as_str(), true, &api_key);
        let payload = GeminiGenerateContentRequest::from_normalized(request);

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;

        let status = response.status().as_u16();
        let retry_after = Self::extract_retry_after(response.headers());

        if status != 200 {
            let body = response.text().await.unwrap_or_default();
            return Err(map_gemini_http_error(status, &body, retry_after));
        }

        let byte_stream = response.bytes_stream().eventsource();
        let chunk_stream = byte_stream.flat_map(|event_result| {
            let chunks: Vec<Result<StreamChunk, AdapterError>> = match event_result {
                Ok(event) => match parse_gemini_stream_chunk(&event.data) {
                    Ok(items) => items.into_iter().map(Ok).collect(),
                    Err(e) => vec![Err(e)],
                },
                Err(e) => vec![Err(AdapterError::Network(e.to_string()))],
            };
            futures::stream::iter(chunks)
        });

        Ok(Box::pin(chunk_stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{FinishReason, Message, ToolDefinition};
    use crate::types::ModelId;

    #[test]
    fn test_contract_gemini_request_serialization() {
        let req = CompletionRequest::new(
            ModelId::new("gemini-1.5-pro").unwrap(),
            vec![
                Message::system("You are a helpful Gemini assistant."),
                Message::user("Hello Gemini!"),
                Message::assistant("Hello there!"),
                Message::tool("calculator", "42"),
            ],
        )
        .with_max_tokens(2048)
        .with_temperature(0.4)
        .with_tools(vec![ToolDefinition::new(
            "calculator",
            "Perform arithmetic",
            serde_json::json!({ "type": "object" }),
        )]);

        let wire_req = GeminiGenerateContentRequest::from_normalized(&req);
        let json = serde_json::to_value(&wire_req).unwrap();

        // System message must be in top-level systemInstruction
        assert!(json["systemInstruction"].is_object());
        let sys_parts = json["systemInstruction"]["parts"].as_array().unwrap();
        assert_eq!(sys_parts[0]["text"], "You are a helpful Gemini assistant.");

        // contents must contain user, model, and tool messages (NOT system)
        let contents = json["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "Hello Gemini!");
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(contents[1]["parts"][0]["text"], "Hello there!");
        assert_eq!(contents[2]["role"], "user");
        assert_eq!(
            contents[2]["parts"][0]["functionResponse"]["name"],
            "calculator"
        );

        // generationConfig
        assert_eq!(json["generationConfig"]["maxOutputTokens"], 2048);
        assert!((json["generationConfig"]["temperature"].as_f64().unwrap() - 0.4).abs() < 1e-4);

        // tools
        let tools = json["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        let decls = tools[0]["functionDeclarations"].as_array().unwrap();
        assert_eq!(decls[0]["name"], "calculator");
    }

    #[test]
    fn test_contract_gemini_response_deserialization() {
        let raw_json = r#"{
            "candidates": [{
                "content": {
                    "parts": [
                        { "text": "I can calculate that for you." },
                        {
                            "functionCall": {
                                "name": "calculator",
                                "args": { "expr": "2+2" }
                            }
                        }
                    ],
                    "role": "model"
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 20,
                "candidatesTokenCount": 15
            }
        }"#;

        let wire_resp: GeminiGenerateContentResponse = serde_json::from_str(raw_json).unwrap();
        let resp = wire_resp.into_normalized().unwrap();

        assert_eq!(resp.content, "I can calculate that for you.");
        assert_eq!(resp.input_tokens, 20);
        assert_eq!(resp.output_tokens, 15);
        assert_eq!(resp.finish_reason, FinishReason::ToolCalls);
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].id, "calculator");
        assert_eq!(resp.tool_calls[0].name, "calculator");
        assert_eq!(resp.tool_calls[0].arguments["expr"], "2+2");
    }

    #[test]
    fn test_contract_gemini_streaming_ndjson_fixture() {
        let ndjson_fixture = [
            r#"{"candidates":[{"content":{"parts":[{"text":"Hello"}],"role":"model"}}]}"#,
            r#"{"candidates":[{"content":{"parts":[{"text":" from"}],"role":"model"}}]}"#,
            r#"{"candidates":[{"content":{"parts":[{"text":" Gemini!"}],"role":"model"},"finishReason":"STOP"}]}"#,
        ];

        let mut chunks = Vec::new();
        for line in ndjson_fixture {
            let parsed = parse_gemini_stream_chunk(line).unwrap();
            chunks.extend(parsed);
        }

        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].delta, "Hello");
        assert!(!chunks[0].done);
        assert_eq!(chunks[1].delta, " from");
        assert!(!chunks[1].done);
        assert_eq!(chunks[2].delta, " Gemini!");
        assert!(!chunks[2].done);
        assert_eq!(chunks[3].delta, "");
        assert!(chunks[3].done);
    }

    #[test]
    fn test_gemini_status_code_mapping() {
        let auth_err = map_gemini_http_error(401, "API key not valid", None);
        assert!(matches!(auth_err, AdapterError::AuthenticationFailed(_)));

        let authz_err = map_gemini_http_error(403, "API key forbidden", None);
        assert!(matches!(authz_err, AdapterError::AuthenticationFailed(_)));

        let rate_err = map_gemini_http_error(429, "quota exhausted", Some(Duration::from_secs(10)));
        assert!(matches!(rate_err, AdapterError::RateLimit { retry_after: Some(d) } if d == Duration::from_secs(10)));

        let svc_err = map_gemini_http_error(503, "backend unavailable", None);
        assert!(matches!(svc_err, AdapterError::ServiceUnavailable));

        let bad_req = map_gemini_http_error(400, "invalid argument", None);
        assert!(matches!(bad_req, AdapterError::InvalidRequest(_)));
    }
}
