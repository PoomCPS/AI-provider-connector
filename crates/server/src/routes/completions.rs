use std::convert::Infallible;
use std::time::Instant;
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Json,
};
use futures::stream::StreamExt;
use serde::Deserialize;
use serde_json::json;
use tracing::{info_span, Span};
use ai_provider_connector_core::{
    AdapterError, CompletionRequest, CompletionResponse, ConnectorError, ProviderKind,
};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ServerCompletionRequest {
    #[serde(default, alias = "provider")]
    pub provider_id: Option<String>,
    #[serde(flatten)]
    pub request: CompletionRequest,
}

pub struct ServerError {
    pub status: StatusCode,
    pub body: serde_json::Value,
    pub retry_after: Option<u64>,
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let mut headers = HeaderMap::new();
        if let Some(sec) = self.retry_after {
            headers.insert(header::RETRY_AFTER, sec.to_string().parse().unwrap());
        }
        (self.status, headers, Json(self.body)).into_response()
    }
}

fn map_connector_error(err: ConnectorError, provider_name: &str) -> ServerError {
    match err {
        ConnectorError::MissingApiKey(_) => ServerError {
            status: StatusCode::FORBIDDEN,
            body: json!({
                "error": "provider_key_not_configured",
                "provider": provider_name
            }),
            retry_after: None,
        },
        ConnectorError::RetriesExhausted { attempts, .. } => ServerError {
            status: StatusCode::BAD_GATEWAY,
            body: json!({
                "error": "retries_exhausted",
                "message": format!("Upstream retries exhausted after {} attempts", attempts)
            }),
            retry_after: None,
        },
        ConnectorError::Adapter(adapter_err) => match adapter_err {
            AdapterError::AuthenticationFailed(msg) => ServerError {
                status: StatusCode::UNAUTHORIZED,
                body: json!({
                    "error": "authentication_failed",
                    "message": msg
                }),
                retry_after: None,
            },
            AdapterError::AuthorizationFailed(msg) => ServerError {
                status: StatusCode::FORBIDDEN,
                body: json!({
                    "error": "authorization_failed",
                    "message": msg
                }),
                retry_after: None,
            },
            AdapterError::RateLimit { retry_after } => ServerError {
                status: StatusCode::TOO_MANY_REQUESTS,
                body: json!({
                    "error": "rate_limit",
                    "message": "Rate limit exceeded"
                }),
                retry_after: retry_after.map(|d| d.as_secs()),
            },
            AdapterError::ServiceUnavailable => ServerError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                body: json!({
                    "error": "service_unavailable",
                    "message": "Upstream service is currently unavailable"
                }),
                retry_after: None,
            },
            AdapterError::InvalidRequest(msg) => ServerError {
                status: StatusCode::BAD_REQUEST,
                body: json!({
                    "error": "invalid_request",
                    "message": msg
                }),
                retry_after: None,
            },
            AdapterError::Http { status, message, retry_after } => ServerError {
                status: StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
                body: json!({
                    "error": "http_error",
                    "message": message
                }),
                retry_after: retry_after.map(|d| d.as_secs()),
            },
            AdapterError::Serialization(msg)
            | AdapterError::Network(msg)
            | AdapterError::Other(msg) => ServerError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: json!({
                    "error": "internal_error",
                    "message": msg
                }),
                retry_after: None,
            },
        },
        ConnectorError::SecretStore(e) => ServerError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: json!({
                "error": "secret_store_error",
                "message": e.to_string()
            }),
            retry_after: None,
        },
        ConnectorError::NotImplemented(_) => ServerError {
            status: StatusCode::BAD_REQUEST,
            body: json!({
                "error": "provider_not_implemented",
                "provider": provider_name
            }),
            retry_after: None,
        },
    }
}

fn resolve_provider(
    provider_id: Option<&str>,
    model: &str,
) -> Result<(ProviderKind, &'static str), ServerError> {
    if let Some(id) = provider_id {
        match id.to_lowercase().as_str() {
            "openai" | "openai-compatible" | "open_ai" => Ok((ProviderKind::OpenAICompatible, "openai")),
            "anthropic" | "claude" => Ok((ProviderKind::Anthropic, "anthropic")),
            "gemini" | "google" => Ok((ProviderKind::Gemini, "gemini")),
            other => Err(ServerError {
                status: StatusCode::BAD_REQUEST,
                body: json!({
                    "error": "invalid_provider",
                    "message": format!("unknown provider_id: {}", other)
                }),
                retry_after: None,
            }),
        }
    } else if model.starts_with("claude") {
        Ok((ProviderKind::Anthropic, "anthropic"))
    } else if model.starts_with("gemini") {
        Ok((ProviderKind::Gemini, "gemini"))
    } else {
        Ok((ProviderKind::OpenAICompatible, "openai"))
    }
}

/// Handler for POST /v1/completions
pub async fn completions_handler(
    State(state): State<AppState>,
    Json(payload): Json<ServerCompletionRequest>,
) -> Result<Json<CompletionResponse>, ServerError> {
    let (provider_kind, provider_name) =
        resolve_provider(payload.provider_id.as_deref(), payload.request.model.as_str())?;

    // Check key configured
    let has_key = state
        .secret_store
        .get(provider_kind)
        .ok()
        .flatten()
        .is_some();
    if !has_key {
        return Err(ServerError {
            status: StatusCode::FORBIDDEN,
            body: json!({
                "error": "provider_key_not_configured",
                "provider": provider_name
            }),
            retry_after: None,
        });
    }

    let span = info_span!(
        "completion",
        provider = %provider_name,
        model = %payload.request.model.as_str(),
        status_code = tracing::field::Empty,
        latency_ms = tracing::field::Empty,
        input_tokens = tracing::field::Empty,
        output_tokens = tracing::field::Empty,
    );
    let _enter = span.enter();

    let start = Instant::now();
    let connector = state
        .get_connector(provider_kind, payload.request.model.clone())
        .map_err(|e| map_connector_error(e, provider_name))?;

    match connector.complete(&payload.request).await {
        Ok(resp) => {
            let elapsed = start.elapsed();
            Span::current().record("status_code", 200);
            Span::current().record("latency_ms", elapsed.as_millis() as u64);
            Span::current().record("input_tokens", resp.input_tokens);
            Span::current().record("output_tokens", resp.output_tokens);
            Ok(Json(resp))
        }
        Err(err) => {
            let elapsed = start.elapsed();
            let server_err = map_connector_error(err, provider_name);
            Span::current().record("status_code", server_err.status.as_u16());
            Span::current().record("latency_ms", elapsed.as_millis() as u64);
            Err(server_err)
        }
    }
}

/// Handler for POST /v1/completions/stream
pub async fn completions_stream_handler(
    State(state): State<AppState>,
    Json(mut payload): Json<ServerCompletionRequest>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, ServerError> {
    payload.request.stream = true;
    let (provider_kind, provider_name) =
        resolve_provider(payload.provider_id.as_deref(), payload.request.model.as_str())?;

    // Check key configured
    let has_key = state
        .secret_store
        .get(provider_kind)
        .ok()
        .flatten()
        .is_some();
    if !has_key {
        return Err(ServerError {
            status: StatusCode::FORBIDDEN,
            body: json!({
                "error": "provider_key_not_configured",
                "provider": provider_name
            }),
            retry_after: None,
        });
    }

    let span = info_span!(
        "completion_stream",
        provider = %provider_name,
        model = %payload.request.model.as_str(),
        status_code = tracing::field::Empty,
        latency_ms = tracing::field::Empty,
    );
    let _enter = span.enter();

    let start = Instant::now();
    let connector = state
        .get_connector(provider_kind, payload.request.model.clone())
        .map_err(|e| map_connector_error(e, provider_name))?;

    let stream = connector
        .stream(&payload.request)
        .await
        .map_err(|e| {
            let server_err = map_connector_error(e, provider_name);
            Span::current().record("status_code", server_err.status.as_u16());
            Span::current().record("latency_ms", start.elapsed().as_millis() as u64);
            server_err
        })?;

    Span::current().record("status_code", 200);

    let mapped = stream.filter_map(|item| async move {
        match item {
            Ok(chunk) => {
                let json_str = serde_json::to_string(&chunk).ok()?;
                Some(Ok(Event::default().data(json_str)))
            }
            Err(_) => None,
        }
    });
    let done = futures::stream::once(async { Ok(Event::default().data("[DONE]")) });
    let event_stream = mapped.chain(done);

    Ok(Sse::new(event_stream).keep_alive(KeepAlive::default()))
}
