// ./src/util.rs
use crate::error::{SwarmError, SwarmResult};
/// Utility functions for the Swarm library
///
/// This module provides various helper functions for debugging, message handling,
/// XML processing, and function conversion utilities.
use crate::types::{AgentFunction, Message, RetryStrategy, Steps};
use quick_xml::de::from_str as xml_from_str;
use regex::Regex;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;
use std::time::Duration;

/// Prints debug messages when debug mode is enabled
///
/// Prefixes debug messages with the text `DEBUG` for easy identification in logs.
///
/// # Arguments
///
/// * `debug` - Boolean flag to enable/disable debug output
/// * `message` - The message to print
///
///
pub fn debug_print(debug: bool, message: &str) {
    if debug {
        tracing::debug!("{}", message);
    }
}

/// Merges a delta message chunk into an existing message
///
/// Used for handling streaming responses where message content arrives in chunks.
/// Updates both content and function calls in the existing message. Particularly
/// useful when processing streaming responses from the OpenAI API.
///
/// # Arguments
///
/// * `message` - The existing message to update
/// * `delta` - The new chunk of message data to merge
///
///
pub fn merge_chunk_message(message: &mut Message, delta: &serde_json::Map<String, Value>) {
    for (key, value) in delta {
        match key.as_str() {
            "content" => {
                if let Some(content) = value.as_str() {
                    message.append_content_fragment(content);
                }
            }
            "function_call" => {
                message.merge_function_call_delta(value);
            }
            _ => {}
        }
    }
}

/// Converts an AgentFunction to a JSON representation
///
/// Transforms a function definition into the format expected by the OpenAI API.
///
/// # Arguments
///
/// * `func` - The AgentFunction to convert
///
/// # Returns
///
/// Returns a Result containing the JSON Value representation of the function
///
/// # Errors
///
/// Will return an error if JSON serialization fails
///
///
pub fn function_to_json(func: &AgentFunction) -> SwarmResult<Value> {
    Ok(json!({
        "name": func.name(),
        "description": func.description(),
        "parameters": func.parameters_schema(),
    }))
}

/// Parses XML content into a Steps structure
///
/// Converts XML-formatted step definitions into a structured Steps object
/// that can be executed by the agent.
///
/// # Arguments
///
/// * `xml_content` - The XML string containing step definitions
///
/// # Returns
///
/// Returns a Result containing the parsed Steps structure
///
/// # Errors
///
/// Will return an error if:
/// * XML parsing fails
/// * Required attributes are missing
/// * Step structure is invalid
///
/// # Examples
///
pub fn parse_steps_from_xml(xml_content: &str) -> SwarmResult<Steps> {
    let steps: Steps = xml_from_str(xml_content)
        .map_err(|e| SwarmError::XmlError(format!("Failed to parse XML steps: {}", e)))?;
    for step in &steps.steps {
        if step.prompt.trim().is_empty() {
            return Err(SwarmError::ValidationError(format!(
                "Step {} has an empty prompt",
                step.number
            )));
        }
    }
    Ok(steps)
}

/// Extracts XML step definitions from instructions text
///
/// Searches for and extracts XML step definitions from a larger text,
/// returning both the instructions without XML and the extracted XML content.
///
/// # Arguments
///
/// * `instructions` - The full instructions text containing potential XML steps
///
/// # Returns
///
/// Returns a tuple containing:
/// * The instructions text with XML removed
/// * Optional XML content if found
///
/// # Errors
///
/// Will return an error if:
/// * Regex pattern is invalid
/// * XML content is malformed
///
///
pub fn extract_xml_steps(instructions: &str) -> SwarmResult<(String, Option<String>)> {
    static STEPS_RE: OnceLock<Regex> = OnceLock::new();
    let re = STEPS_RE.get_or_init(|| {
        Regex::new(r"(?s)<steps\b[^>]*>.*?</steps>").expect("static steps regex must compile")
    });

    let mut instructions_without_xml = instructions.to_string();
    let mut xml_steps = None;

    if let Some(mat) = re.find(instructions) {
        let xml_content = mat.as_str();
        instructions_without_xml.replace_range(mat.range(), "");
        xml_steps = Some(xml_content.to_string());
    }

    Ok((instructions_without_xml.trim().to_string(), xml_steps))
}

/// Truncates a string to at most `max_len` **bytes**, appending "…" if truncated.
///
/// The actual cut point may be ≤ `max_len` bytes when the byte at `max_len` falls
/// inside a multi-byte UTF-8 sequence; the function always cuts on a char boundary.
///
/// Used in Display impls to prevent accidental leakage of API keys, tokens,
/// or PII from error messages into logs.
pub fn safe_truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let truncate_at = (0..=max_len)
            .rev()
            .find(|&i| s.is_char_boundary(i))
            .unwrap_or(0);
        format!("{}…", &s[..truncate_at])
    }
}

/// Retries an async operation according to the given [`RetryStrategy`].
///
/// Only retries when [`SwarmError::is_retriable`] returns `true`. Uses
/// exponential backoff capped at `strategy.max_delay`.
///
/// # Example
/// ```rust,ignore
/// let result = with_retry(&config.api_settings.retry_strategy, || {
///     Box::pin(client.call())
/// }).await?;
/// ```
pub async fn with_retry<F, T>(strategy: &RetryStrategy, mut f: F) -> SwarmResult<T>
where
    F: FnMut() -> Pin<Box<dyn Future<Output = SwarmResult<T>> + 'static>>,
{
    let mut delay = strategy.initial_delay();
    for attempt in 0..=strategy.max_retries() {
        match f().await {
            Ok(value) => return Ok(value),
            Err(err) if attempt < strategy.max_retries() && err.is_retriable() => {
                tracing::warn!(
                    "Retryable error on attempt {}/{}, retrying in {}ms: {}",
                    attempt + 1,
                    strategy.max_retries(),
                    delay.as_millis(),
                    err
                );
                tokio::time::sleep(delay).await;
                let next_ms = (delay.as_millis() as f64 * strategy.backoff_factor() as f64) as u64;
                delay = Duration::from_millis(next_ms.min(strategy.max_delay().as_millis() as u64));
            }
            Err(err) => return Err(err),
        }
    }
    Err(SwarmError::Other("Retry attempts exhausted".to_string()))
}
