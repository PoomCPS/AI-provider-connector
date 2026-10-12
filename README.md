# AI Provider Connector

A unified interface for connecting multiple AI providers (OpenAI-compatible, Anthropic, Gemini) with type-level security, credential management, retry/backoff, and a local dashboard.

## Workspace Structure

The project is organized as a Cargo workspace with two crates:

- **`crates/core`** (`ai-provider-connector-core`): Library crate providing the normalized `Connector` facade, `ProviderAdapter` trait, validated newtypes (`ApiKey`, `EndpointUrl`, `ModelId`), and secret storage.
- **`crates/server`** (`ai-provider-connector-server`): Binary crate providing an `axum`-based HTTP server (REST + SSE API) and local dashboard UI.

## Getting Started

```bash
# Check compilation across all crates
cargo check --workspace

# Run tests
cargo test --workspace

# Run server
cargo run -p ai-provider-connector-server
```
