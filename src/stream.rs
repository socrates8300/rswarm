// File: rswarm/src/stream.rs

use async_stream::try_stream;
use futures_util::{stream::Stream, StreamExt};
use reqwest::Client;
use serde_json::{json, Value};

use crate::error::{SwarmError, SwarmResult};
use crate::types::{Agent, ChatCompletionResponse, ContextVariables, Instructions, Message};
use crate::util::{debug_print, function_to_json};

/// Streamer provides a streaming–based API to receive agent responses incrementally.
pub struct Streamer {
    client: Client,
    api_key: String,
    api_url: String,
}

impl Streamer {
    /// Create a new Streamer instance using the provided HTTP Client, API key, and API URL.
    pub fn new(client: Client, api_key: String, api_url: String) -> Self {
        Self {
            client,
            api_key,
            api_url,
        }
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
        debug_print(debug, &format!("stream called with debug={:?}", debug));
        let history_vec = history.to_vec();
        let system_instructions = match &agent.instructions {
            Instructions::Text(text) => text.clone(),
            Instructions::Function(func) => func(context_variables.clone()),
        };
        // Pre-compute fallible values so ? can be used inside try_stream!
        let functions_result: SwarmResult<Vec<Value>> =
            agent.functions.iter().map(function_to_json).collect();
        let function_call_json: Option<Value> =
            agent.function_call().to_wire_value().map(|s| json!(s));

        let api_url = self.api_url.clone();

        // Use try_stream to create a stream that can yield items and errors.
        try_stream! {
            // Build messages, propagating any construction errors.
            let system_msg = Message::system(system_instructions)?;
            let mut messages = vec![system_msg];
            messages.extend_from_slice(&history_vec);

            // Build functions list, propagating serialization errors.
            let functions = functions_result?;

            // Build the request payload.
            let mut request_body = json!({
                "model": model,
                "messages": messages,
                "stream": true,
            });
            if !functions.is_empty() {
                request_body["functions"] = Value::Array(functions);
            }
            if let Some(fc) = function_call_json {
                request_body["function_call"] = fc;
            }

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
                            if let Some(stripped) = line.strip_prefix("data: ") {
                                let json_str = stripped.trim();
                                if json_str == "[DONE]" {
                                    break;
                                }
                                // Deserialize the partial response.
                                let partial: ChatCompletionResponse = serde_json::from_str(json_str)
                                    .map_err(|e| SwarmError::DeserializationError(e.to_string()))?;
                                // Yield each message.
                                for choice in partial.into_choices() {
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
