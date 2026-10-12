//! Wire format and JSON structures for OpenAI-compatible Chat Completions API.

use std::time::Duration;
use serde::{Deserialize, Serialize};
use crate::adapter::AdapterError;
use crate::protocol::{
    CompletionRequest, CompletionResponse, FinishReason, Message, StreamChunk, ToolCall,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAIChatRequest {
    pub model: String,
    pub messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAITool>>,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "role")]
pub enum OpenAIMessage {
    #[serde(rename = "system")]
    System { content: String },
    #[serde(rename = "user")]
    User { content: String },
    #[serde(rename = "assistant")]
    Assistant {
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<OpenAIToolCall>>,
    },
    #[serde(rename = "tool")]
    Tool {
        tool_call_id: String,
        content: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAITool {
    pub r#type: String,
    pub function: OpenAIFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAIFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAIToolCall {
    pub id: String,
    pub r#type: String,
    pub function: OpenAIFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAIFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAIChatResponse {
    pub choices: Vec<OpenAIChoice>,
    #[serde(default)]
    pub usage: Option<OpenAIUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAIChoice {
    pub message: OpenAIChoiceMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAIChoiceMessage {
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAIUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAIStreamChunk {
    #[serde(default)]
    pub choices: Vec<OpenAIStreamChoice>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAIStreamChoice {
    #[serde(default)]
    pub delta: OpenAIStreamDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct OpenAIStreamDelta {
    pub content: Option<String>,
}

impl OpenAIChatRequest {
    pub fn from_normalized(request: &CompletionRequest) -> Self {
        let messages = request
            .messages
            .iter()
            .map(|msg| match msg {
                Message::System(c) => OpenAIMessage::System { content: c.clone() },
                Message::User(c) => OpenAIMessage::User { content: c.clone() },
                Message::Assistant(c) => OpenAIMessage::Assistant {
                    content: Some(c.clone()),
                    tool_calls: None,
                },
                Message::Tool { call_id, content } => OpenAIMessage::Tool {
                    tool_call_id: call_id.clone(),
                    content: content.clone(),
                },
            })
            .collect();

        let tools = if request.tools.is_empty() {
            None
        } else {
            Some(
                request
                    .tools
                    .iter()
                    .map(|t| OpenAITool {
                        r#type: "function".to_string(),
                        function: OpenAIFunction {
                            name: t.name.clone(),
                            description: t.description.clone(),
                            parameters: t.parameters.clone(),
                        },
                    })
                    .collect(),
            )
        };

        Self {
            model: request.model.as_str().to_string(),
            messages,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            tools,
            stream: request.stream,
        }
    }
}

impl OpenAIChatResponse {
    pub fn into_normalized(self) -> Result<CompletionResponse, AdapterError> {
        let choice = self.choices.into_iter().next().ok_or_else(|| {
            AdapterError::Serialization("missing choices in OpenAI response".into())
        })?;

        let content = choice.message.content.unwrap_or_default();

        let tool_calls = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tc| {
                let arguments = serde_json::from_str(&tc.function.arguments)
                    .unwrap_or(serde_json::Value::String(tc.function.arguments));
                ToolCall::new(tc.id, tc.function.name, arguments)
            })
            .collect();

        let finish_reason = match choice.finish_reason.as_deref() {
            Some("stop") => FinishReason::Stop,
            Some("length") => FinishReason::Length,
            Some("tool_calls") => FinishReason::ToolCalls,
            Some(other) => FinishReason::Other(other.to_string()),
            None => FinishReason::Stop,
        };

        let (input_tokens, output_tokens) = match self.usage {
            Some(u) => (u.prompt_tokens.unwrap_or(0), u.completion_tokens.unwrap_or(0)),
            None => (0, 0),
        };

        Ok(CompletionResponse {
            content,
            input_tokens,
            output_tokens,
            finish_reason,
            tool_calls,
        })
    }
}

/// Decodes a single SSE event or line from OpenAI streaming output.
pub fn parse_openai_sse_data(data: &str) -> Result<Option<StreamChunk>, AdapterError> {
    let trimmed = data.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed == "[DONE]" {
        return Ok(Some(StreamChunk::done()));
    }

    let parsed: OpenAIStreamChunk = serde_json::from_str(trimmed)
        .map_err(|e| AdapterError::Serialization(format!("failed to parse SSE data: {e}")))?;

    if let Some(choice) = parsed.choices.into_iter().next() {
        if let Some(content) = choice.delta.content {
            return Ok(Some(StreamChunk::token(content)));
        }
    }

    Ok(None)
}

/// Maps HTTP status codes to typed AdapterError variants.
pub fn map_openai_http_error(status: u16, body: &str, retry_after: Option<Duration>) -> AdapterError {
    match status {
        401 => AdapterError::AuthenticationFailed(body.to_string()),
        403 => AdapterError::AuthorizationFailed(body.to_string()),
        429 => AdapterError::RateLimit { retry_after },
        503 => AdapterError::ServiceUnavailable,
        400..=499 => AdapterError::InvalidRequest(body.to_string()),
        _ => AdapterError::Http {
            status,
            message: body.to_string(),
            retry_after,
        },
    }
}
