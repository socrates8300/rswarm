// File: rswarm/src/stream.rs

use async_stream::try_stream;
use futures_util::{stream::Stream, StreamExt};
use reqwest::Client;
use serde_json::{json, Value};
use std::env;

use crate::constants::{OPENAI_DEFAULT_API_URL, ROLE_SYSTEM};
use crate::error::{SwarmError, SwarmResult};
use crate::types::{Agent, ChatCompletionResponse, ContextVariables, Instructions, Message};
use crate::util::function_to_json;

/// Streamer provides a streaming–based API to receive agent responses incrementally.
pub struct Streamer {
    client: Client,
    api_key: String,
}

impl Streamer {
    /// Create a new Streamer instance using the provided HTTP Client and API key.
    pub fn new(client: Client, api_key: String) -> Self {
        Self { client, api_key }
    }

    /// Begins a streaming chat completion request.
    ///
    /// The returned stream yields individual messages (using a JSON structure
    /// defined by ChatCompletionResponse) as soon as they are available.

    pub fn stream_chat(
        &self,
        agent: &Agent,
        history: &[Message],
        context_variables: &ContextVariables,
        model_override: Option<String>,
        debug: bool,
    ) -> impl Stream<Item = SwarmResult<Message>> {
        // Clone values to use in the async block.
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let model = model_override.unwrap_or_else(|| match &agent.instructions {
            Instructions::Text(_text) => agent.model.clone(),
            Instructions::Function(_func) => agent.model.clone(),
        });
        println!("{:?}", debug);
        // Build the messages vector.
        let mut messages = Vec::new();
        let system_instructions = match &agent.instructions {
            Instructions::Text(text) => text.clone(),
            Instructions::Function(func) => func(context_variables.clone()),
        };
        messages.push(Message {
            role: ROLE_SYSTEM.to_string(),
            content: Some(system_instructions),
            name: None,
            function_call: None,
        });
        messages.extend_from_slice(history);

        // Prepare any functions for the API.
        let functions: Vec<Value> = agent
            .functions
            .iter()
            .map(|func| function_to_json(func))
            .collect::<SwarmResult<Vec<Value>>>()
            .unwrap_or_default();

        // Build the request payload.
        let mut request_body = json!({
            "model": model,
            "messages": messages,
            "stream": true,
        });
        if !functions.is_empty() {
            request_body["functions"] = Value::Array(functions);
        }

        // Determine API URL from environment or default.
        let api_url =
            env::var("OPENAI_API_URL").unwrap_or_else(|_| OPENAI_DEFAULT_API_URL.to_string());

        // Use try_stream to create a stream that can yield items and errors.
        try_stream! {
            // Send POST request.
            let response = client
                .post(api_url)
                .bearer_auth(api_key)
                .json(&request_body)
                .send()
                .await
                .map_err(|e| SwarmError::NetworkError(e.to_string()))?;

            // Ensure the HTTP status is successful without consuming response.
            response.error_for_status_ref()
                .map_err(|e| SwarmError::ApiError(e.to_string()))?;

            // Now get the streaming body.
            let mut byte_stream = response.bytes_stream();
            while let Some(chunk_result) = byte_stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        let text_chunk = String::from_utf8_lossy(&chunk);
                        // Each line is expected to be prefixed with "data: ".
                        for line in text_chunk.lines() {
                            if line.starts_with("data: ") {
                                let json_str = line[6..].trim();
                                if json_str == "[DONE]" {
                                    break;
                                }
                                // Deserialize the partial response.
                                let partial: ChatCompletionResponse = serde_json::from_str(json_str)
                                    .map_err(|e| SwarmError::DeserializationError(e.to_string()))?;
                                // Yield each message.
                                for choice in partial.choices {
                                    yield choice.message;
                                }
                            }
                        }
                    }
                    Err(e) => Err(SwarmError::StreamError(e.to_string()))?,
                }
            }
        }
    }
}
