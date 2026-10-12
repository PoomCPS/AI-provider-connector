//! Wire types and JSON serialization for Anthropic Messages API.

use std::time::Duration;
use serde::{Deserialize, Serialize};
use crate::adapter::AdapterError;
use crate::protocol::{
    CompletionRequest, CompletionResponse, FinishReason, Message, StreamChunk, ToolCall,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicMessagesRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub messages: Vec<AnthropicMessage>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicTool>>,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: AnthropicMessageContent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum AnthropicMessageContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicMessagesResponse {
    pub content: Vec<AnthropicResponseContentBlock>,
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub usage: Option<AnthropicUsage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicResponseContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicStreamEvent {
    pub r#type: String,
    #[serde(default)]
    pub delta: Option<AnthropicStreamDelta>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicStreamDelta {
    pub r#type: Option<String>,
    pub text: Option<String>,
}

impl AnthropicMessagesRequest {
    pub fn from_normalized(request: &CompletionRequest) -> Self {
        let mut system_parts = Vec::new();
        let mut messages = Vec::new();

        for msg in &request.messages {
            match msg {
                Message::System(sys) => system_parts.push(sys.clone()),
                Message::User(u) => messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicMessageContent::Text(u.clone()),
                }),
                Message::Assistant(a) => messages.push(AnthropicMessage {
                    role: "assistant".to_string(),
                    content: AnthropicMessageContent::Text(a.clone()),
                }),
                Message::Tool { call_id, content } => messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicMessageContent::Blocks(vec![AnthropicContentBlock::ToolResult {
                        tool_use_id: call_id.clone(),
                        content: content.clone(),
                    }]),
                }),
            }
        }

        let system = if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n\n"))
        };

        let tools = if request.tools.is_empty() {
            None
        } else {
            Some(
                request
                    .tools
                    .iter()
                    .map(|t| AnthropicTool {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        input_schema: t.parameters.clone(),
                    })
                    .collect(),
            )
        };

        Self {
            model: request.model.as_str().to_string(),
            system,
            messages,
            max_tokens: request.max_tokens.unwrap_or(4096),
            temperature: request.temperature,
            tools,
            stream: request.stream,
        }
    }
}

impl AnthropicMessagesResponse {
    pub fn into_normalized(self) -> Result<CompletionResponse, AdapterError> {
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for block in self.content {
            match block {
                AnthropicResponseContentBlock::Text { text } => text_parts.push(text),
                AnthropicResponseContentBlock::ToolUse { id, name, input } => {
                    tool_calls.push(ToolCall::new(id, name, input));
                }
            }
        }

        let finish_reason = match self.stop_reason.as_deref() {
            Some("end_turn") | Some("stop_sequence") => FinishReason::Stop,
            Some("max_tokens") => FinishReason::Length,
            Some("tool_use") => FinishReason::ToolCalls,
            Some(other) => FinishReason::Other(other.to_string()),
            None => FinishReason::Stop,
        };

        let (input_tokens, output_tokens) = match self.usage {
            Some(u) => (u.input_tokens.unwrap_or(0), u.output_tokens.unwrap_or(0)),
            None => (0, 0),
        };

        Ok(CompletionResponse {
            content: text_parts.join(""),
            input_tokens,
            output_tokens,
            finish_reason,
            tool_calls,
        })
    }
}

/// Decodes an Anthropic SSE event string given its event name and data.
pub fn parse_anthropic_sse_event(event_name: Option<&str>, data: &str) -> Result<Option<StreamChunk>, AdapterError> {
    let trimmed = data.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if event_name == Some("message_stop") {
        return Ok(Some(StreamChunk::done()));
    }

    let parsed: AnthropicStreamEvent = serde_json::from_str(trimmed)
        .map_err(|e| AdapterError::Serialization(format!("failed to parse Anthropic SSE: {e}")))?;

    if parsed.r#type == "message_stop" {
        return Ok(Some(StreamChunk::done()));
    }

    if parsed.r#type == "content_block_delta" {
        if let Some(delta) = parsed.delta {
            if let Some(text) = delta.text {
                return Ok(Some(StreamChunk::token(text)));
            }
        }
    }

    Ok(None)
}

/// Maps Anthropic HTTP error codes to AdapterError.
pub fn map_anthropic_http_error(status: u16, body: &str, retry_after: Option<Duration>) -> AdapterError {
    match status {
        401 => AdapterError::AuthenticationFailed(body.to_string()),
        403 => AdapterError::AuthorizationFailed(body.to_string()),
        429 => AdapterError::RateLimit { retry_after },
        529 | 503 => AdapterError::ServiceUnavailable,
        400..=499 => AdapterError::InvalidRequest(body.to_string()),
        _ => AdapterError::Http {
            status,
            message: body.to_string(),
            retry_after,
        },
    }
}
