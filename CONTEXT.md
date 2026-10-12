# AI Provider Connector

A reusable Rust workspace that normalizes requests across multiple AI providers (OpenAI-compatible, Anthropic, Google Gemini) behind a single typed interface. Consumed either as a direct Rust library dependency or as a locally-running HTTP service from Docker, Python, Node, or any other language.

## Language

### Request / Response Domain

**CompletionRequest**:
A normalized, provider-agnostic description of a single inference call: a message history, optional tool definitions, and sampling parameters. Caller constructs one; the crate routes it to the correct provider.
_Avoid_: prompt, query, inference request, API call

**CompletionResponse**:
The normalized result of a completed inference: the assistant message, token usage, finish reason, and optional tool calls. Always returned regardless of which provider processed the request.
_Avoid_: answer, reply, result, output

**StreamChunk**:
A single incremental fragment delivered during a streaming CompletionRequest. Carries a delta string and a terminal flag. Collected by the caller or accumulated by the crate.
_Avoid_: token, chunk, delta, SSE event

**ToolCall**:
A provider-requested invocation of a caller-registered function: a name, an id, and JSON arguments. Part of a CompletionResponse when the model decides to call a tool.
_Avoid_: function call, action, plugin call

**ToolResult**:
The caller's response to a ToolCall: the same id, and the serialized output the model should see before continuing.
_Avoid_: function result, tool output, tool response

### Providers

**Provider**:
A concrete AI service the crate can dispatch to: OpenAI-compatible (OpenAI, Ollama, DeepSeek, Groq, OpenRouter, LM Studio), Anthropic, or Google Gemini. Each provider has exactly one Adapter.
_Avoid_: backend, service, vendor, model host

**Adapter**:
The internal implementation that translates normalized requests into a specific provider's wire protocol and normalizes the response back. One Adapter per Provider; they are never exposed to callers.
_Avoid_: client, connector, driver, implementation, wrapper

**ProviderKind**:
The discriminator that identifies which Provider to use: `OpenAICompatible`, `Anthropic`, `Gemini`. Selected at configuration time, never at call time.
_Avoid_: provider name, backend type, model type

### Credentials and Security

**ApiKey**:
A validated newtype wrapping a provider credential string. Can only be constructed if the string passes length bounds, character safety, and HTTP-header compatibility checks. Never logged.
_Avoid_: token, secret, password, credential string

**SecretStore**:
The trait a caller implements to supply and persist ApiKey values. The crate reads keys through this seam and writes updated keys back through it. Never owns storage itself.
_Avoid_: key store, credential store, secrets manager, vault

**KeyringSecretStore**:
The built-in SecretStore adapter that reads and writes ApiKey values via the OS keychain (macOS Keychain, Windows Credential Manager, libsecret on Linux).
_Avoid_: OS store, system store, native store

**TlsConfig**:
The caller-injectable TLS configuration attached to a Provider at construction time. Defaults to system root certificates. Optionally carries a pinned certificate store for high-security deployments.
_Avoid_: SSL config, certificate config, TLS settings

### Network and Routing

**ConnectorConfig**:
The complete, validated construction-time configuration for a Provider connection: ProviderKind, endpoint URL, model, SecretStore reference, TlsConfig, and retry policy. Immutable after construction.
_Avoid_: settings, options, config bag, provider config

**EndpointUrl**:
A validated newtype wrapping a provider base URL. Validated at construction: must be HTTPS (except localhost), no path component beyond a base prefix, no user-info component.
_Avoid_: base URL, API URL, server URL

**RetryPolicy**:
The backoff and jitter strategy applied to transient failures (HTTP 429, 503, network errors). Honors `Retry-After` response headers. Caller configures max attempts and base delay.
_Avoid_: backoff, retry config, error recovery

### Server

**ConnectorServer**:
The optional `axum`-based HTTP binary that exposes the `core` library over a local REST + SSE API. Started as a standalone process; other projects connect to it over `localhost:PORT` or a Docker network.
_Avoid_: proxy, gateway, sidecar, HTTP wrapper

**Dashboard**:
The web UI served by the ConnectorServer for managing provider configuration and ApiKey values. Follows the Swiss Design Gráfico design system from `design.md`. Not a public API; local management only.
_Avoid_: admin panel, settings page, web app

**ConnectorServerPort**:
The configured TCP port the ConnectorServer binds to. Default `6173`. Callers and Docker projects discover it through environment variable `AI_CONNECTOR_PORT`.
_Avoid_: port number, listen port, server port
