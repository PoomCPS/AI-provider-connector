# Spec: AI Provider Connector — Phase 1 MVP

## Problem Statement

Building a new AI-powered project currently means writing the same provider integration code from scratch every time: HTTP clients for OpenAI, Anthropic, and Gemini each have different wire formats, different auth schemes, different error shapes, and different streaming protocols. API keys need to be stored securely, rotated without redeployment, and managed from a UI. Projects written in Python, Node, or running in Docker can't reuse Rust code anyway. The result is duplicated, insecure, hard-to-maintain integration glue in every project.

## Solution

A Rust Cargo workspace with two crates:

1. **`core`** — A Rust library crate that presents a single normalized `Connector` interface for sending `CompletionRequest`s and receiving `CompletionResponse`s (or streamed `StreamChunk`s) across three provider families: OpenAI-compatible endpoints, Anthropic, and Google Gemini. Security is built into the type system: `ApiKey`, `EndpointUrl`, and `ModelId` are validated newtypes; credentials flow through a `SecretStore` trait backed by the OS keychain. Retry and backoff are built in.

2. **`server`** — A binary crate that wraps `core` in an `axum`-based local HTTP server, exposing a REST + SSE API and a web dashboard (Swiss Design Gráfico visual system) for managing provider keys and testing completions. Other projects — Docker containers, Python scripts, Node apps — connect over `localhost:6173` or a Docker network without knowing Rust.

## User Stories

### Rust Library Consumer

1. As a Rust developer, I want to add `ai-provider-connector-core` as a Cargo dependency, so that I can call any AI provider through one typed interface.
2. As a Rust developer, I want to construct a `Connector` from a `ConnectorConfig`, so that all validation (key, endpoint, model) fails at construction time, never mid-request.
3. As a Rust developer, I want to call `connector.complete(req)` and receive a normalized `CompletionResponse`, so that I never have to parse provider-specific JSON myself.
4. As a Rust developer, I want to call `connector.stream(req)` and receive a `Stream<Item = Result<StreamChunk>>`, so that I can render tokens incrementally without polling.
5. As a Rust developer, I want to pass an `OpenAICompatible` `ProviderKind` with any endpoint URL, so that I can use Ollama, DeepSeek, Groq, OpenRouter, or LM Studio with zero extra code.
6. As a Rust developer, I want to pass an `Anthropic` `ProviderKind`, so that I can use Claude models with their native Messages API.
7. As a Rust developer, I want to pass a `Gemini` `ProviderKind`, so that I can use Gemini models with their native REST API.
8. As a Rust developer, I want to include `ToolCall` definitions in my `CompletionRequest`, so that the model can invoke my functions.
9. As a Rust developer, I want to include `ToolResult` messages in my follow-up `CompletionRequest`, so that the model sees my function's output and continues.
10. As a Rust developer, I want `401` and `403` errors to surface immediately without retry, so that I'm told about invalid keys without waiting through backoff.
11. As a Rust developer, I want `429` and `503` errors to be retried with jitter and `Retry-After` header respect, so that transient quota exhaustion is handled transparently.
12. As a Rust developer, I want to inject an `InMemorySecretStore` in tests, so that I never need real API keys in CI.
13. As a Rust developer, I want `ApiKey::new("bad\x00key")` to return an error at compile-visible construction time, so that control characters and oversized keys can never reach a provider.
14. As a Rust developer, I want to inject a `TlsConfig` with pinned root certificates, so that I can deploy in high-security environments that prohibit OS certificate stores.

### Dashboard / Server Consumer

15. As a project owner, I want to run `ai-provider-connector-server` as a binary or Docker container, so that non-Rust projects can use the connector over HTTP.
16. As a project owner, I want to open `http://localhost:6173/dashboard` in a browser, so that I can see all configured providers and their key status at a glance.
17. As a project owner, I want to type an API key into the Dashboard and click "Set Key", so that the key is saved to the OS keychain immediately without restarting the server.
18. As a project owner, I want to click "Remove Key" in the Dashboard, so that the key is deleted from the keychain and the provider becomes inactive.
19. As a project owner, I want the Dashboard to mask the stored key (show only last 4 chars), so that my API key is never visible in plaintext in the browser.
20. As a Python developer, I want to POST to `http://localhost:6173/v1/completions` with a JSON body, so that I can use any AI provider without writing auth or retry logic.
21. As a Python developer, I want to POST to `http://localhost:6173/v1/completions/stream` and read SSE events, so that I can stream responses token-by-token in my app.
22. As a Docker Compose user, I want to run `ai-provider-connector-server` as a service and reference it from other containers by hostname, so that all my containers share one key store.
23. As any consumer, I want `GET /health` to return a 200 with provider status, so that orchestration tools (Kubernetes, Docker Compose) can liveness-check the service.
24. As any consumer, I want `GET /v1/providers` to list providers with `key_set: true/false`, so that I can tell at a glance which providers are ready without revealing keys.
25. As an operator, I want to configure the server port via `AI_CONNECTOR_PORT` environment variable, so that I can avoid port conflicts in multi-service deployments.

### Security

26. As a security-conscious user, I want API keys to never appear in application logs, so that log aggregation systems cannot leak credentials.
27. As a security-conscious user, I want prompt content and response content to never appear in tracing spans, so that PII in prompts stays out of observability tooling.
28. As a security-conscious user, I want the server to bind to `127.0.0.1` by default and require explicit opt-in for `0.0.0.0`, so that the dashboard is not accidentally exposed on a public interface.

## Implementation Decisions

### Workspace Structure
A Cargo workspace with two crates: `crates/core` (library, no axum dependency) and `crates/server` (binary, depends on core). Downstream Rust projects depend only on `core`; Docker users run `server`. This boundary is enforced: axum is never a dependency of `core`.

### The Primary Seam: `ProviderAdapter` trait
```rust
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    async fn complete(&self, request: &CompletionRequest) -> Result<CompletionResponse, AdapterError>;
    async fn stream(&self, request: &CompletionRequest) -> Result<BoxStream<'static, Result<StreamChunk, AdapterError>>, AdapterError>;
}
```
This is the single seam across the codebase. `Connector` holds an `Arc<dyn ProviderAdapter>`. Callers never see individual adapter types. (From architecture grilling session.)

### Type-level Validation Newtypes
`ApiKey`, `EndpointUrl`, and `ModelId` are opaque newtypes constructed only through validated constructors. `ApiKey::new()` rejects control chars, enforces max 4096 bytes, and verifies HTTP header compatibility. `EndpointUrl::new()` enforces HTTPS (except `localhost`), rejects user-info components. These types are the only way to construct a `ConnectorConfig`; there is no escape hatch.

### `SecretStore` Trait for Key Management
```rust
pub trait SecretStore: Send + Sync {
    fn get(&self, provider: ProviderKind) -> Result<Option<ApiKey>, SecretStoreError>;
    fn set(&self, provider: ProviderKind, key: ApiKey) -> Result<(), SecretStoreError>;
    fn delete(&self, provider: ProviderKind) -> Result<(), SecretStoreError>;
}
```
Three built-in adapters: `KeyringSecretStore` (OS keychain via `keyring` crate), `EnvSecretStore` (read-only from environment variables), `InMemorySecretStore` (tests). The Dashboard calls `set` on key save, `delete` on key removal.

### Provider Adapters
Three provider families, each with identical internal structure (adapter, wire types, SSE decoder):
- **OpenAI-compatible**: single adapter handles OpenAI, Ollama, DeepSeek, Groq, OpenRouter, LM Studio by accepting a configurable `EndpointUrl`
- **Anthropic**: native Messages API with `anthropic-version` header, `input_tokens`/`output_tokens` in response
- **Gemini**: Google REST API with API key as query param, `candidates[0].content` response shape

### Retry Policy
Exponential backoff: `delay = min(base_delay * 2^attempt, max_delay)` with ±25% jitter. Only retries `429` (honor `Retry-After` header if present) and `503`. Never retries `401`, `403`, `400`, or `404`. `RetryPolicy` is configurable per `ConnectorConfig`. Default: 3 attempts, 500ms base, 30s cap.

### TLS
Default: system root certificates via `rustls-native-certs`. `TlsConfig` accepts an optional `RootCertStore` for certificate pinning. The `rustls` provider is installed once at process startup via `rustls::crypto::ring::default_provider().install_default()`.

### Observability (No PII)
All `tracing` spans emit: provider kind, model id, HTTP status, latency ms, input/output token counts, attempt number. Content fields (`messages`, `response text`, tool arguments) are explicitly excluded from all spans and events. No debug-level content logging exists in v1.

### Server API
`axum` binary in `crates/server`. Binds to `127.0.0.1:6173` by default; `AI_CONNECTOR_PORT` overrides port; `AI_CONNECTOR_HOST=0.0.0.0` opts into all-interface binding. Serves:
- `POST /v1/completions` — JSON request/response
- `POST /v1/completions/stream` — SSE response
- `GET /v1/providers` — list provider status
- `POST /v1/providers/{id}/key` — set key (body: `{ "key": "..." }`)
- `DELETE /v1/providers/{id}/key` — remove key
- `GET /health` — liveness
- `GET /dashboard` — serves Dashboard HTML

### Dashboard Visual System
From `design.md` (Swiss Design Gráfico): Signal Red `#D82C2C`, Black `#1A1A1A`, Deep Blue `#0033A0`, Cool Grey `#A9B2B1`, White `#FFFFFF`. Helvetica Neue for UI, JetBrains Mono for API key fields and model IDs. Asymmetric grid layout. No decorative gradients, no emojis, icons from Lucide only.

## Testing Decisions

**Good tests** test the `ProviderAdapter` trait and `Connector` through their public interface only. They do not reach into wire encoding or HTTP client internals directly.

**What to test:**
- `ApiKey::new()` validation: control chars, too long, header-unsafe chars — all return typed errors
- `EndpointUrl::new()` validation: HTTP (non-localhost) rejected, user-info rejected, malformed URLs rejected
- `Connector::complete()` with `InMemorySecretStore` + a mock `ProviderAdapter` that returns a canned `CompletionResponse`
- `RetryPolicy` behavior: asserts that a mock adapter returning `429` is called up to `max_attempts` times before surfacing the error, and that a `401` is not retried
- Each provider adapter has contract tests asserting that a `CompletionRequest` round-trips correctly through its wire encoding (no live network calls in CI)
- `SecretStore` implementations: `InMemorySecretStore` set/get/delete round-trips; `EnvSecretStore` reads the correct env var
- Server routes: `axum::test` integration tests asserting correct HTTP status codes for each route with an `InMemorySecretStore`

**Prior art (from Mimi-live):** `translation_contract_tests.rs` pattern in `shared/mimi-runtime/src/clients/` — contract tests per provider verifying wire encoding without live network access.

## Out of Scope

- **Phase 2 / `realtime` feature**: WebSocket duplex streaming (Gemini Live, OpenAI Realtime) — out of scope for Phase 1
- **Chinese cloud providers**: Volcano Engine, Tencent Cloud, Baidu Translate, Alibaba — out of scope
- **Audio/speech recognition**: out of scope
- **Embeddings API**: out of scope for v1 (different return type, not a `CompletionResponse`)
- **Image generation**: out of scope
- **`EnvSecretStore` write support**: environment variables are read-only by design
- **Remote deployment / TLS termination for the server**: server is local-only in v1; production TLS reverse proxy is the caller's problem
- **Token bucket client-side rate limiter**: not in v1; add if 429s are observed in practice

## Further Notes

The `design.md` file at the repo root contains the full Swiss Design Gráfico design token set and component specification. The Dashboard implementation should be derived directly from it — treat `design.md` as the single source of truth for all visual decisions; do not introduce ad-hoc colors or spacing.

The workspace follows the same two-layer pattern as `Mimi-live` (`shared/mimi-core` + `shared/mimi-runtime`) but simplifies it: `core` is a single crate rather than two, because this project has no OS-native code requiring a separate crate boundary.
