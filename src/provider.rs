use crate::error::SwarmError;
use crate::tool::ToolSchema;
use crate::types::Message;
use async_trait::async_trait;
use futures::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, SwarmError>;
    async fn stream(
        &self,
        request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Chunk, SwarmError>> + Send>>, SwarmError>;
    fn model_name(&self) -> &str;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub messages: Vec<Message>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolSchema>>,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
}

impl CompletionRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            messages,
            model: model.into(),
            tools: None,
            stream: false,
            temperature: None,
            max_tokens: None,
            stop: None,
        }
    }

    pub fn with_tools(mut self, tools: Vec<ToolSchema>) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn with_stream(mut self, stream: bool) -> Self {
        self.stream = stream;
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn with_stop(mut self, stop: Vec<String>) -> Self {
        self.stop = Some(stop);
        self
    }

    pub fn validate(&self) -> Result<(), SwarmError> {
        if self.messages.is_empty() {
            return Err(SwarmError::ValidationError(
                "CompletionRequest.messages cannot be empty".to_string(),
            ));
        }
        for message in &self.messages {
            message.validate()?;
        }
        if self.model.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "CompletionRequest.model cannot be empty".to_string(),
            ));
        }
        if let Some(temperature) = self.temperature {
            if !temperature.is_finite() || !(0.0..=2.0).contains(&temperature) {
                return Err(SwarmError::ValidationError(
                    "CompletionRequest.temperature must be between 0.0 and 2.0".to_string(),
                ));
            }
        }
        if let Some(max_tokens) = self.max_tokens {
            if max_tokens == 0 {
                return Err(SwarmError::ValidationError(
                    "CompletionRequest.max_tokens must be greater than 0".to_string(),
                ));
            }
        }
        if let Some(stop) = &self.stop {
            if stop.is_empty() || stop.iter().any(|sequence| sequence.trim().is_empty()) {
                return Err(SwarmError::ValidationError(
                    "CompletionRequest.stop cannot contain empty sequences".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub fn build(self) -> Result<Self, SwarmError> {
        self.validate()?;
        Ok(self)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<CompletionChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<CompletionUsage>,
}

impl CompletionResponse {
    pub fn text(&self) -> Option<String> {
        self.choices.first().and_then(|c| c.message.content.clone())
    }

    pub fn tool_calls(&self) -> Option<Vec<ToolCallInResponse>> {
        self.choices.first().and_then(|c| c.message.tool_calls.clone())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompletionChoice {
    pub index: u32,
    pub message: CompletionMessage,
    #[serde(rename = "finish_reason")]
    pub finish_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompletionMessage {
    pub role: CompletionRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallInResponse>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompletionRole {
    System,
    User,
    Assistant,
    Tool,
    Function,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCallInResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: ToolCallKind,
    pub function: ToolCallFunction,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallKind {
    Function,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompletionUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkChoice {
    pub index: u32,
    pub delta: ChunkDelta,
    pub finish_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<CompletionRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCallDelta {
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub call_type: Option<ToolCallKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<ToolCallFunctionDelta>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCallFunctionDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

/// Concrete [`LlmProvider`] implementation backed by the OpenAI-compatible
/// chat completions API.
///
/// Uses the `tools` schema (modern OpenAI API). For legacy `functions`-style
/// calls use `Swarm::get_chat_completion` directly until migration is complete.
pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    api_url: String,
}

impl OpenAiProvider {
    pub fn new(
        client: Client,
        api_key: impl Into<String>,
        api_url: impl Into<String>,
    ) -> Self {
        Self {
            client,
            api_key: api_key.into(),
            api_url: api_url.into(),
        }
    }

    pub fn api_url(&self) -> &str {
        &self.api_url
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, SwarmError> {
        request.validate()?;

        let response = self
            .client
            .post(&self.api_url)
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await
            .map_err(|e| SwarmError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(SwarmError::ApiError(text));
        }

        let text = response
            .text()
            .await
            .map_err(|e| SwarmError::DeserializationError(e.to_string()))?;

        serde_json::from_str(&text).map_err(|e| {
            SwarmError::DeserializationError(format!(
                "Failed to parse CompletionResponse: {}",
                e
            ))
        })
    }

    async fn stream(
        &self,
        _request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Chunk, SwarmError>> + Send>>, SwarmError> {
        // SSE streaming via OpenAiProvider is deferred.
        // Use Swarm::get_chat_completion with stream=true for SSE responses.
        Err(SwarmError::Other(
            "OpenAiProvider streaming not yet implemented; use Swarm::get_chat_completion"
                .to_string(),
        ))
    }

    fn model_name(&self) -> &str {
        // Model selection is per-request via CompletionRequest.model.
        &self.api_url
    }
}
