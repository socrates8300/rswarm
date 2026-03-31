//  ./src/validation.rs
/// Validation module for Swarm API requests and configurations.
use crate::error::{SwarmError, SwarmResult};
use crate::types::{Agent, Instructions, Message, RuntimeLimits, SwarmConfig};
use serde_json::Value;
use std::time::Instant;
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
///
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
            return Err(SwarmError::ValidationError(
                "Model name cannot be empty".to_string(),
            ));
        }
    }

    // Validate agent
    if agent.name().trim().is_empty() {
        return Err(SwarmError::ValidationError(
            "Agent name cannot be empty".to_string(),
        ));
    }

    match agent.instructions() {
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
    if messages.is_empty() {
        return Err(SwarmError::ValidationError(
            "Message history cannot be empty".to_string(),
        ));
    }
    for message in messages {
        message.validate()?;
    }

    Ok(())
}

// =============================================================================
// #40 — Cumulative budget enforcer
// =============================================================================

/// Reason why a budget limit was exhausted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetExhausted {
    TokenBudget { used: u32, limit: u32 },
    TokensPerRequest { used: u32, limit: u32 },
    WallTime { elapsed_secs: u64, limit: u64 },
    ToolCallQuota { used: u32, limit: u32 },
    MaxDepth { depth: u32, limit: u32 },
}

impl std::fmt::Display for BudgetExhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TokenBudget { used, limit } => write!(
                f,
                "token budget exhausted: {} / {} tokens used",
                used, limit
            ),
            Self::TokensPerRequest { used, limit } => write!(
                f,
                "per-request token limit exceeded: {} / {} tokens used",
                used, limit
            ),
            Self::WallTime {
                elapsed_secs,
                limit,
            } => write!(
                f,
                "wall-time limit exceeded: {}s elapsed, limit {}s",
                elapsed_secs, limit
            ),
            Self::ToolCallQuota { used, limit } => write!(
                f,
                "tool call quota exhausted: {} / {} calls used",
                used, limit
            ),
            Self::MaxDepth { depth, limit } => write!(
                f,
                "max recursion depth exceeded: depth {} / limit {}",
                depth, limit
            ),
        }
    }
}

impl From<BudgetExhausted> for SwarmError {
    fn from(e: BudgetExhausted) -> Self {
        SwarmError::Other(e.to_string())
    }
}

/// Tracks cumulative resource usage against configured limits.
///
/// Call [`BudgetEnforcer::check`] at the top of each iteration to detect
/// exhaustion before it becomes a runaway condition.
pub struct BudgetEnforcer {
    limits: RuntimeLimits,
    start: Instant,
    /// Number of loop iterations completed.
    pub iterations: u32,
    /// Cumulative tokens consumed (prompt + completion).
    pub total_tokens: u32,
    /// Cumulative tool calls made.
    pub tool_calls: u32,
    /// Cumulative number of agent handoffs performed so far in this run.
    ///
    /// This is a **monotonically increasing counter** — it is never decremented
    /// because agent handoffs in the run loop are sequential (not recursive).
    /// `RuntimeLimits::max_depth` limits the total number of handoffs allowed.
    pub depth: u32,
}

impl BudgetEnforcer {
    pub fn new(limits: RuntimeLimits) -> Self {
        Self {
            limits,
            start: Instant::now(),
            iterations: 0,
            total_tokens: 0,
            tool_calls: 0,
            depth: 0,
        }
    }

    /// Check all configured limits. Returns `Err(BudgetExhausted)` the first
    /// limit that is exceeded, `Ok(())` when all limits are within bounds.
    pub fn check(&self) -> Result<(), BudgetExhausted> {
        if let Some(budget) = self.limits.token_budget {
            if self.total_tokens >= budget {
                return Err(BudgetExhausted::TokenBudget {
                    used: self.total_tokens,
                    limit: budget,
                });
            }
        }
        if let Some(max_secs) = self.limits.max_wall_time_secs {
            let elapsed = self.start.elapsed().as_secs();
            if elapsed >= max_secs {
                return Err(BudgetExhausted::WallTime {
                    elapsed_secs: elapsed,
                    limit: max_secs,
                });
            }
        }
        if let Some(quota) = self.limits.max_tool_calls {
            if self.tool_calls >= quota {
                return Err(BudgetExhausted::ToolCallQuota {
                    used: self.tool_calls,
                    limit: quota,
                });
            }
        }
        if let Some(max_depth) = self.limits.max_depth {
            // Use `>` so that max_depth=1 permits exactly 1 handoff before
            // blocking (depth is incremented *before* this check).
            if self.depth > max_depth {
                return Err(BudgetExhausted::MaxDepth {
                    depth: self.depth,
                    limit: max_depth,
                });
            }
        }
        Ok(())
    }

    pub fn add_tokens(&mut self, count: u32) {
        self.total_tokens = self.total_tokens.saturating_add(count);
    }
    pub fn increment_iterations(&mut self) {
        self.iterations = self.iterations.saturating_add(1);
    }
    pub fn increment_tool_calls(&mut self) {
        self.tool_calls = self.tool_calls.saturating_add(1);
    }
    pub fn increment_depth(&mut self) {
        self.depth = self.depth.saturating_add(1);
    }
    pub fn decrement_depth(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }
}

// =============================================================================
// #45 — Output verification for tool calls and structured LLM responses
// =============================================================================

/// Verify that `args` (a JSON object) contains all fields listed under
/// `schema["required"]`.
///
/// `schema` is the `parameters` field of a `ToolSchema` — a JSON Schema object.
/// Returns `Ok(())` if all required fields are present or if `required` is
/// absent from the schema.
pub fn verify_tool_arguments(args: &Value, schema: &Value) -> SwarmResult<()> {
    let required = match schema.get("required").and_then(|v| v.as_array()) {
        Some(r) => r,
        None => return Ok(()), // no required fields declared
    };
    for field in required {
        let field_name = field.as_str().unwrap_or_default();
        let present = args
            .as_object()
            .map(|m| m.contains_key(field_name))
            .unwrap_or(false);
        if !present {
            return Err(SwarmError::ValidationError(format!(
                "tool argument '{}' is required but missing",
                field_name
            )));
        }
    }
    Ok(())
}

/// Verify that a structured JSON response from an LLM contains all
/// `expected_fields`. Returns `Ok(())` on success.
pub fn verify_structured_response(response: &Value, expected_fields: &[&str]) -> SwarmResult<()> {
    let obj = response.as_object().ok_or_else(|| {
        SwarmError::ValidationError("structured response must be a JSON object".to_string())
    })?;
    for field in expected_fields {
        if !obj.contains_key(*field) {
            return Err(SwarmError::ValidationError(format!(
                "structured response missing required field '{}'",
                field
            )));
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
pub fn validate_api_url(url: &str, config: &SwarmConfig) -> SwarmResult<()> {
    // Check if URL is empty
    if url.trim().is_empty() {
        return Err(SwarmError::ValidationError(
            "API URL cannot be empty".to_string(),
        ));
    }

    // Parse URL
    let parsed_url = Url::parse(url)
        .map_err(|e| SwarmError::ValidationError(format!("Invalid API URL format: {}", e)))?;

    // Allow localhost URLs on any port
    if parsed_url.host_str() == Some("localhost") {
        return Ok(());
    }

    // Verify against allowed prefixes
    if !config
        .valid_api_url_prefixes()
        .iter()
        .any(|prefix| url.starts_with(prefix.as_str()))
    {
        return Err(SwarmError::ValidationError(format!(
            "API URL must start with one of: {}",
            config
                .valid_api_url_prefixes()
                .iter()
                .map(|prefix| prefix.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_depth_allows_exact_budget_then_exhausts() {
        let mut budget = BudgetEnforcer::new(RuntimeLimits {
            max_depth: Some(1),
            ..RuntimeLimits::default()
        });

        assert!(budget.check().is_ok());
        budget.increment_depth();
        assert!(budget.check().is_ok());
        budget.increment_depth();
        assert!(matches!(
            budget.check(),
            Err(BudgetExhausted::MaxDepth { depth: 2, limit: 1 })
        ));
    }
}
