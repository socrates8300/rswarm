use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::types::{AgentFunction, ContextVariables, ResultType};

#[async_trait]
pub trait Tool: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    async fn execute(&self, args: InvocationArgs) -> Result<Value, ToolError>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ToolError {
    Validation(String),
    Execution(String),
    Timeout { duration_ms: u64 },
    Network(String),
    NotFound(String),
}

impl ToolError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, ToolError::Timeout { .. } | ToolError::Network(_))
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(msg) => write!(f, "Validation: {}", msg),
            Self::Execution(msg) => write!(f, "Execution: {}", msg),
            Self::Timeout { duration_ms } => write!(f, "Timeout: {}ms", duration_ms),
            Self::Network(msg) => write!(f, "Network: {}", msg),
            Self::NotFound(name) => write!(f, "NotFound: {}", name),
        }
    }
}

impl std::error::Error for ToolError {}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct InvocationArgs(Value);

impl InvocationArgs {
    pub fn from_value(value: Value) -> Result<Self, ToolError> {
        if value.is_null() {
            return Err(ToolError::Validation(
                "Invocation arguments cannot be null".to_string(),
            ));
        }
        Ok(Self(value))
    }

    pub fn from_json_str(raw: &str) -> Result<Self, ToolError> {
        let value: Value = serde_json::from_str(raw)
            .map_err(|error| ToolError::Validation(format!("Invalid JSON arguments: {}", error)))?;
        Self::from_value(value)
    }

    pub fn as_value(&self) -> &Value {
        &self.0
    }

    pub fn into_value(self) -> Value {
        self.0
    }

    pub fn as_str(&self) -> Option<&str> {
        self.0.as_str()
    }

    pub fn as_bool(&self) -> Option<bool> {
        self.0.as_bool()
    }

    pub fn as_f64(&self) -> Option<f64> {
        self.0.as_f64()
    }

    pub fn as_object(&self) -> Option<&Map<String, Value>> {
        self.0.as_object()
    }

    pub fn to_context_variables(&self) -> Result<ContextVariables, ToolError> {
        let object = self.as_object().ok_or_else(|| {
            ToolError::Validation(
                "Agent functions require object-shaped invocation arguments".to_string(),
            )
        })?;

        object
            .iter()
            .map(|(key, value)| {
                let value = match value {
                    Value::String(text) => text.clone(),
                    Value::Number(number) => number.to_string(),
                    Value::Bool(boolean) => boolean.to_string(),
                    Value::Null => {
                        return Err(ToolError::Validation(format!(
                            "Argument '{}' cannot be null when converting to context variables",
                            key
                        )))
                    }
                    Value::Array(_) | Value::Object(_) => {
                        serde_json::to_string(value).map_err(|error| {
                            ToolError::Validation(format!(
                                "Failed to serialize nested argument '{}': {}",
                                key, error
                            ))
                        })?
                    }
                };
                Ok((key.clone(), value))
            })
            .collect()
    }

    pub fn validate_against_schema(&self, schema: &Value) -> Result<(), ToolError> {
        jsonschema::validate(schema, &self.0).map_err(|e| ToolError::Validation(e.to_string()))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolSchema {
    pub fn from_tool<T: Tool + ?Sized>(tool: &T) -> Self {
        let description = tool.description().to_string();
        if description.is_empty() {
            tracing::warn!(
                tool = %tool.name(),
                "ToolSchema: tool '{}' has no description — LLM may misuse it",
                tool.name()
            );
        }
        Self {
            name: tool.name().to_string(),
            description,
            parameters: tool.parameters_schema(),
        }
    }

    pub fn validate_args(&self, args: &InvocationArgs) -> Result<(), ToolError> {
        args.validate_against_schema(&self.parameters)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCallSpec {
    id: String,
    name: String,
    #[serde(rename = "arguments")]
    pub args: InvocationArgs,
}

impl ToolCallSpec {
    pub fn new(name: impl Into<String>, args: Value) -> Result<Self, ToolError> {
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            args: InvocationArgs::from_value(args)?,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn args(&self) -> &InvocationArgs {
        &self.args
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum ToolResult {
    Success {
        call_id: String,
        name: String,
        result: Value,
        duration_ms: u64,
    },
    Failure {
        call_id: String,
        name: String,
        error: String,
        duration_ms: u64,
    },
}

impl ToolResult {
    pub fn success(
        call_id: impl Into<String>,
        name: impl Into<String>,
        result: Value,
        duration_ms: u64,
    ) -> Self {
        Self::Success {
            call_id: call_id.into(),
            name: name.into(),
            result,
            duration_ms,
        }
    }

    pub fn failure(
        call_id: impl Into<String>,
        name: impl Into<String>,
        error: String,
        duration_ms: u64,
    ) -> Self {
        Self::Failure {
            call_id: call_id.into(),
            name: name.into(),
            error,
            duration_ms,
        }
    }

    pub fn call_id(&self) -> &str {
        match self {
            Self::Success { call_id, .. } | Self::Failure { call_id, .. } => call_id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Success { name, .. } | Self::Failure { name, .. } => name,
        }
    }

    pub fn duration_ms(&self) -> u64 {
        match self {
            Self::Success { duration_ms, .. } | Self::Failure { duration_ms, .. } => *duration_ms,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    pub fn result(&self) -> Option<&Value> {
        match self {
            Self::Success { result, .. } => Some(result),
            Self::Failure { .. } => None,
        }
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Success { .. } => None,
            Self::Failure { error, .. } => Some(error),
        }
    }
}

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register<T: Tool + 'static>(&mut self, tool: T) -> &mut Self {
        self.tools.insert(tool.name().to_string(), Arc::new(tool));
        self
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn list_all(&self) -> Vec<Arc<dyn Tool>> {
        self.tools.values().cloned().collect()
    }

    pub fn to_openai_functions(&self) -> Vec<Value> {
        self.tools
            .values()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "parameters": tool.parameters_schema()
                })
            })
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Adapts a closure-based [`AgentFunction`] to the [`Tool`] trait.
///
/// This bridge allows existing `AgentFunction` registrations to be dispatched
/// via the `Tool` trait without requiring callers to rewrite their functions.
pub struct ClosureTool {
    name: String,
    description: String,
    agent_fn: AgentFunction,
}

impl ClosureTool {
    pub fn from_agent_function(agent_fn: AgentFunction) -> Self {
        let name = agent_fn.name().to_string();
        Self {
            name,
            description: String::new(),
            agent_fn,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
}

#[async_trait]
impl Tool for ClosureTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, args: InvocationArgs) -> Result<Value, ToolError> {
        let ctx: ContextVariables = args
            .to_context_variables()
            .map_err(|e| ToolError::Validation(e.to_string()))?;

        let result = (self.agent_fn.function)(ctx)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        Ok(match result {
            ResultType::Value(s) => Value::String(s),
            ResultType::Agent(agent) => serde_json::json!({ "agent_handoff": agent.name() }),
            ResultType::ContextVariables(ctx_vars) => {
                serde_json::to_value(ctx_vars).unwrap_or(Value::Null)
            }
            ResultType::Termination(reason) => {
                serde_json::json!({ "termination": reason.to_string() })
            }
        })
    }
}
