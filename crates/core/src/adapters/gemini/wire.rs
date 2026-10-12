//! Wire types and JSON serialization for Google Gemini REST API.

use std::time::Duration;
use serde::{Deserialize, Serialize};
use crate::adapter::AdapterError;
use crate::protocol::{
    CompletionRequest, CompletionResponse, FinishReason, Message, StreamChunk, ToolCall,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerateContentRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GeminiContent>,
    pub contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GeminiTool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeminiContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub parts: Vec<GeminiPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_response: Option<GeminiFunctionResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<GeminiFunctionCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeminiFunctionResponse {
    pub name: String,
    pub response: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeminiFunctionCall {
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiTool {
    pub function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeminiFunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerateContentResponse {
    #[serde(default)]
    pub candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    pub usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiCandidate {
    pub content: Option<GeminiContent>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiUsageMetadata {
    pub prompt_token_count: Option<u32>,
    pub candidates_token_count: Option<u32>,
}

impl GeminiGenerateContentRequest {
    pub fn from_normalized(request: &CompletionRequest) -> Self {
        let mut system_parts = Vec::new();
        let mut contents = Vec::new();

        for msg in &request.messages {
            match msg {
                Message::System(sys) => {
                    system_parts.push(GeminiPart {
                        text: Some(sys.clone()),
                        function_response: None,
                        function_call: None,
                    });
                }
                Message::User(u) => {
                    contents.push(GeminiContent {
                        role: Some("user".to_string()),
                        parts: vec![GeminiPart {
                            text: Some(u.clone()),
                            function_response: None,
                            function_call: None,
                        }],
                    });
                }
                Message::Assistant(a) => {
                    contents.push(GeminiContent {
                        role: Some("model".to_string()),
                        parts: vec![GeminiPart {
                            text: Some(a.clone()),
                            function_response: None,
                            function_call: None,
                        }],
                    });
                }
                Message::Tool { call_id, content } => {
                    contents.push(GeminiContent {
                        role: Some("user".to_string()),
                        parts: vec![GeminiPart {
                            text: None,
                            function_response: Some(GeminiFunctionResponse {
                                name: call_id.clone(),
                                response: serde_json::json!({ "output": content }),
                            }),
                            function_call: None,
                        }],
                    });
                }
            }
        }

        let system_instruction = if system_parts.is_empty() {
            None
        } else {
            Some(GeminiContent {
                role: None,
                parts: system_parts,
            })
        };

        let generation_config = if request.max_tokens.is_some() || request.temperature.is_some() {
            Some(GeminiGenerationConfig {
                max_output_tokens: request.max_tokens,
                temperature: request.temperature,
            })
        } else {
            None
        };

        let tools = if request.tools.is_empty() {
            None
        } else {
            Some(vec![GeminiTool {
                function_declarations: request
                    .tools
                    .iter()
                    .map(|t| GeminiFunctionDeclaration {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        parameters: t.parameters.clone(),
                    })
                    .collect(),
            }])
        };

        Self {
            system_instruction,
            contents,
            generation_config,
            tools,
        }
    }
}

impl GeminiGenerateContentResponse {
    pub fn into_normalized(self) -> Result<CompletionResponse, AdapterError> {
        let candidate = self.candidates.into_iter().next().ok_or_else(|| {
            AdapterError::Serialization("missing candidates in Gemini response".into())
        })?;

        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();

        if let Some(content) = candidate.content {
            for part in content.parts {
                if let Some(text) = part.text {
                    text_parts.push(text);
                }
                if let Some(fc) = part.function_call {
                    tool_calls.push(ToolCall::new(fc.name.clone(), fc.name, fc.args));
                }
            }
        }

        let finish_reason = match candidate.finish_reason.as_deref() {
            Some("STOP") => {
                if !tool_calls.is_empty() {
                    FinishReason::ToolCalls
                } else {
                    FinishReason::Stop
                }
            }
            Some("MAX_TOKENS") => FinishReason::Length,
            Some(other) => FinishReason::Other(other.to_string()),
            None => {
                if !tool_calls.is_empty() {
                    FinishReason::ToolCalls
                } else {
                    FinishReason::Stop
                }
            }
        };

        let (input_tokens, output_tokens) = match self.usage_metadata {
            Some(u) => (
                u.prompt_token_count.unwrap_or(0),
                u.candidates_token_count.unwrap_or(0),
            ),
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

/// Decodes a NDJSON or SSE line from Gemini streaming response.
pub fn parse_gemini_stream_chunk(line: &str) -> Result<Vec<StreamChunk>, AdapterError> {
    let mut trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    if let Some(rest) = trimmed.strip_prefix("data: ") {
        trimmed = rest.trim();
    }

    if trimmed == "[DONE]" {
        return Ok(vec![StreamChunk::done()]);
    }

    let parsed: GeminiGenerateContentResponse = serde_json::from_str(trimmed)
        .map_err(|e| AdapterError::Serialization(format!("failed to parse Gemini stream chunk: {e}")))?;

    let mut result = Vec::new();

    if let Some(candidate) = parsed.candidates.into_iter().next() {
        if let Some(content) = candidate.content {
            for part in content.parts {
                if let Some(text) = part.text {
                    if !text.is_empty() {
                        result.push(StreamChunk::token(text));
                    }
                }
            }
        }

        if candidate.finish_reason.is_some() {
            result.push(StreamChunk::done());
        }
    }

    Ok(result)
}

/// Maps Gemini HTTP error status codes to AdapterError.
pub fn map_gemini_http_error(status: u16, body: &str, retry_after: Option<Duration>) -> AdapterError {
    match status {
        401 | 403 => AdapterError::AuthenticationFailed(body.to_string()),
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
