//! Normalized protocol types for completions, streaming, and tool calls.

use crate::types::ModelId;
use serde::{Deserialize, Serialize};

/// Role or type of message in a completion conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", content = "content", rename_all = "lowercase")]
pub enum Message {
    System(String),
    User(String),
    Assistant(String),
    #[serde(rename = "tool")]
    Tool {
        call_id: String,
        content: String,
    },
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self::System(content.into())
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::User(content.into())
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::Assistant(content.into())
    }

    pub fn tool(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::Tool {
            call_id: call_id.into(),
            content: content.into(),
        }
    }
}

/// JSON Schema definition for a callable tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl ToolDefinition {
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }
}

/// A tool invocation returned by the AI provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

impl ToolCall {
    pub fn new(id: impl Into<String>, name: impl Into<String>, arguments: serde_json::Value) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments,
        }
    }
}

/// Reason the model finished generating tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FinishReason {
    Stop,
    Length,
    #[serde(rename = "tool_calls")]
    ToolCalls,
    #[serde(untagged)]
    Other(String),
}

/// Normalized completion request sent across any provider adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub messages: Vec<Message>,
    pub model: ModelId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,
    #[serde(default)]
    pub stream: bool,
}

impl CompletionRequest {
    pub fn new(model: ModelId, messages: Vec<Message>) -> Self {
        Self {
            messages,
            model,
            max_tokens: None,
            temperature: None,
            tools: Vec::new(),
            stream: false,
        }
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_stream(mut self, stream: bool) -> Self {
        self.stream = stream;
        self
    }
}

/// Normalized completion response returned from any provider adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionResponse {
    pub content: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub finish_reason: FinishReason,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
}

/// Incremental token chunk received during a streaming completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamChunk {
    pub delta: String,
    pub done: bool,
}

impl StreamChunk {
    pub fn token(delta: impl Into<String>) -> Self {
        Self {
            delta: delta.into(),
            done: false,
        }
    }

    pub fn done() -> Self {
        Self {
            delta: String::new(),
            done: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization() {
        let msg = Message::User("hello world".into());
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"hello world\""));

        let tool_msg = Message::Tool {
            call_id: "call_123".into(),
            content: "result".into(),
        };
        let tool_json = serde_json::to_string(&tool_msg).unwrap();
        assert!(tool_json.contains("\"role\":\"tool\""));
        assert!(tool_json.contains("\"call_id\":\"call_123\""));
    }

    #[test]
    fn test_completion_request_builder() {
        let req = CompletionRequest::new(
            ModelId::new("gpt-4o").unwrap(),
            vec![Message::user("ping")],
        )
        .with_max_tokens(100)
        .with_temperature(0.7)
        .with_stream(true);

        assert_eq!(req.model.as_str(), "gpt-4o");
        assert_eq!(req.max_tokens, Some(100));
        assert_eq!(req.temperature, Some(0.7));
        assert!(req.stream);
    }
}
