use crate::error::{SwarmError, SwarmResult};
use crate::types::{Agent, Instructions, Message};

pub fn validate_api_request(
    agent: &Agent,
    messages: &[Message],
    model: &Option<String>,
    max_turns: usize,
) -> SwarmResult<()> {
    // Validate model
    if let Some(model_name) = model {
        if model_name.trim().is_empty() {
            return Err(SwarmError::ValidationError("Model name cannot be empty".to_string()));
        }
        // Add specific model validation if needed
        if !model_name.starts_with("gpt-") {
            return Err(SwarmError::ValidationError(
                "Invalid model name format. Must start with 'gpt-'".to_string(),
            ));
        }
    }

    // Validate agent
    if agent.name.trim().is_empty() {
        return Err(SwarmError::ValidationError("Agent name cannot be empty".to_string()));
    }

    match &agent.instructions {
        Instructions::Text(text) if text.trim().is_empty() => {
            return Err(SwarmError::ValidationError(
                "Agent instructions cannot be empty".to_string(),
            ))
        }
        _ => {}
    }

    // Validate messages
    for (index, message) in messages.iter().enumerate() {
        if message.role.trim().is_empty() {
            return Err(SwarmError::ValidationError(format!(
                "Message at index {} has empty role",
                index
            )));
        }
        // Validate content if it exists
        if let Some(content) = &message.content {
            if content.trim().is_empty() {
                return Err(SwarmError::ValidationError(format!(
                    "Message at index {} has empty content",
                    index
                )));
            }
        }
    }

    // Validate max_turns
    if max_turns == 0 {
        return Err(SwarmError::ValidationError(
            "max_turns must be greater than 0".to_string(),
        ));
    }

    Ok(())
}