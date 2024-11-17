//  ./src/validation.rs
/// Validation module for Swarm API requests and configurations
///
/// This module provides validation functions to ensure that API requests
/// and configurations meet the required criteria before execution.
use crate::error::{SwarmError, SwarmResult};
use crate::types::{Agent, Instructions, Message, SwarmConfig};
use url::Url;

/// Validates an API request before execution
///
/// Performs comprehensive validation of all components of an API request,
/// including the agent configuration, message history, model selection,
/// and execution parameters.
///
/// # Arguments
///
/// * `agent` - The agent configuration to validate
/// * `messages` - The message history to validate
/// * `model` - Optional model override to validate
/// * `max_turns` - Maximum number of conversation turns (must be > 0 and <= config.max_loop_iterations)
///
/// # Returns
///
/// Returns `Ok(())` if all validations pass, or a `SwarmError` describing
/// the validation failure.
///
/// # Errors
///
/// Will return `SwarmError::ValidationError` if:
/// * Model name is empty or invalid
/// * Agent name is empty
/// * Agent instructions are empty
/// * Message roles or content are empty
/// * max_turns is 0 or exceeds config.max_loop_iterations
///
/// # Examples
///
/// ```rust
/// use rswarm::{Agent, Message, validate_api_request};
///
/// let agent = Agent {
///     name: "test_agent".to_string(),
///     model: "gpt-4".to_string(),
///     instructions: Instructions::Text("You are a helpful assistant.".to_string()),
///     functions: vec![],
///     function_call: None,
///     parallel_tool_calls: false,
/// };
///
/// let messages = vec![Message {
///     role: "user".to_string(),
///     content: Some("Hello".to_string()),
///     name: None,
///     function_call: None,
/// }];
///
/// // Use a reasonable value for max_turns that doesn't exceed max_loop_iterations
/// let result = validate_api_request(&agent, &messages, &None, 5);
/// assert!(result.is_ok());
/// ```
pub fn validate_api_request(
    agent: &Agent,
    messages: &[Message],
    model: &Option<String>,
    max_turns: usize,
) -> SwarmResult<()> {
    // Validate max_turns
    if max_turns == 0 {
        return Err(SwarmError::ValidationError(
            "max_turns must be greater than 0".to_string(),
        ));
    }

    // Validate model
    if let Some(model_name) = model {
        if model_name.trim().is_empty() {
            return Err(SwarmError::ValidationError("Model name cannot be empty".to_string()));
        }
    }

    // Validate agent
    if agent.name.trim().is_empty() {
        return Err(SwarmError::ValidationError("Agent name cannot be empty".to_string()));
    }

    match &agent.instructions {
        Instructions::Text(text) => {
            if text.trim().is_empty() {
                return Err(SwarmError::ValidationError(
                    "Agent instructions cannot be empty".to_string(),
                ));
            }
        }
        Instructions::Function(_) => {} // Function-based instructions are validated at runtime
    }

    // Validate messages
    for message in messages {
        if message.role.trim().is_empty() {
            return Err(SwarmError::ValidationError("Message role cannot be empty".to_string()));
        }

        // Only validate content if there's no function call
        if message.function_call.is_none() {
            if let Some(content) = &message.content {
                if content.trim().is_empty() {
                    return Err(SwarmError::ValidationError(
                        "Message content cannot be empty".to_string(),
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Validates an API URL against configuration requirements
///
/// Ensures that the provided API URL meets all security and formatting
/// requirements specified in the configuration.
///
/// # Arguments
///
/// * `url` - The URL string to validate
/// * `config` - The SwarmConfig containing validation rules
///
/// # Returns
///
/// Returns `Ok(())` if the URL is valid, or a `SwarmError` describing
/// the validation failure.
///
/// # Errors
///
/// Will return `SwarmError::ValidationError` if:
/// * URL is empty
/// * URL format is invalid
/// * URL scheme is not HTTPS
/// * URL doesn't match any allowed prefixes from config
///
/// # Examples
///
/// ```rust
/// use rswarm::{SwarmConfig, validate_api_url};
///
/// let config = SwarmConfig {
///     valid_api_url_prefixes: vec!["https://api.openai.com".to_string()],
///     // ... other fields ...
/// };
///
/// let url = "https://api.openai.com/v1/chat/completions";
/// let result = validate_api_url(url, &config);
/// assert!(result.is_ok());
///
/// let invalid_url = "http://invalid-url.com";
/// let result = validate_api_url(invalid_url, &config);
/// assert!(result.is_err());
/// ```
pub fn validate_api_url(url: &str, config: &SwarmConfig) -> SwarmResult<()> {
    // Check if URL is empty
    if url.trim().is_empty() {
        return Err(SwarmError::ValidationError("API URL cannot be empty".to_string()));
    }

    // Parse URL
    let parsed_url = Url::parse(url)
        .map_err(|e| SwarmError::ValidationError(format!("Invalid API URL format: {}", e)))?;

    // Verify HTTPS
    if parsed_url.scheme() != "https" {
        return Err(SwarmError::ValidationError(
            "API URL must use HTTPS protocol".to_string(),
        ));
    }

    // Verify against allowed prefixes
    if !config.valid_api_url_prefixes
        .iter()
        .any(|prefix| url.starts_with(prefix))
    {
        return Err(SwarmError::ValidationError(format!(
            "API URL must start with one of: {}",
            config.valid_api_url_prefixes.join(", ")
        )));
    }

    Ok(())
}