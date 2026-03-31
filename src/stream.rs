// File: rswarm/src/stream.rs

use async_stream::try_stream;
use futures_util::{stream::Stream, StreamExt};
use reqwest::Client;
use serde_json::{json, Value};

use crate::error::{SwarmError, SwarmResult};
use crate::types::{
    Agent, ApiKey, ContextVariables, FunctionCall, Instructions, Message, MessageRole,
};
use crate::util::{debug_print, function_to_json};

/// Streamer provides a streaming–based API to receive agent responses incrementally.
pub struct Streamer {
    client: Client,
    api_key: ApiKey,
    api_url: String,
}

impl Streamer {
    /// Create a new Streamer instance using the provided HTTP Client, API key, and API URL.
    pub fn new(client: Client, api_key: ApiKey, api_url: String) -> Self {
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
                .bearer_auth(api_key.as_str())
                .json(&request_body)
                .send()
                .await
                .map_err(|e| SwarmError::NetworkError(e.to_string()))?;

            // Ensure the HTTP status is successful without consuming response.
            response.error_for_status_ref()
                .map_err(|e| SwarmError::ApiError(e.to_string()))?;

            // Now get the streaming body.
            let mut byte_stream = response.bytes_stream();
            // Line buffer: TCP chunks can split SSE `data:` lines across boundaries.
            let mut line_buf = String::new();
            'sse: while let Some(chunk_result) = byte_stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        line_buf.push_str(&String::from_utf8_lossy(&chunk));
                        // Process every complete line (terminated by \n).
                        while let Some(newline_pos) = line_buf.find('\n') {
                            let line = line_buf[..newline_pos]
                                .trim_end_matches('\r')
                                .to_string();
                            line_buf.drain(..=newline_pos);

                            if let Some(json_str) = line.strip_prefix("data: ") {
                                let json_str = json_str.trim();
                                if json_str == "[DONE]" {
                                    break 'sse;
                                }
                                // Parse as raw Value to avoid validation failures
                                // on empty-content delta messages.
                                let chunk_val: Value = serde_json::from_str(json_str)
                                    .map_err(|e| SwarmError::DeserializationError(e.to_string()))?;
                                if let Some(choices) = chunk_val["choices"].as_array() {
                                    for choice in choices {
                                        // Real OpenAI SSE uses "delta"; non-streaming uses
                                        // "message". Support both for test/compat.
                                        let null = Value::Null;
                                        let source: &Value = choice
                                            .get("delta")
                                            .or_else(|| choice.get("message"))
                                            .unwrap_or(&null);
                                        let content = source["content"]
                                            .as_str()
                                            .filter(|s| !s.is_empty())
                                            .map(str::to_owned);
                                        let fc_val = source.get("function_call").cloned();
                                        // Only yield when there is actual payload.
                                        if content.is_some() || fc_val.is_some() {
                                            let fc = fc_val.map(|v| {
                                                FunctionCall::from_parts_unchecked(
                                                    v["name"]
                                                        .as_str()
                                                        .unwrap_or("")
                                                        .to_string(),
                                                    v["arguments"]
                                                        .as_str()
                                                        .unwrap_or("")
                                                        .to_string(),
                                                )
                                            });
                                            yield Message::from_parts_unchecked(
                                                MessageRole::Assistant,
                                                content,
                                                None,
                                                fc,
                                            );
                                        }
                                    }
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
