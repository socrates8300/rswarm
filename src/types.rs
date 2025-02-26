// File: rswarm/src/types.rs

use crate::constants::{
    DEFAULT_API_VERSION, DEFAULT_CONNECT_TIMEOUT, DEFAULT_REQUEST_TIMEOUT, OPENAI_DEFAULT_API_URL,
    VALID_API_URL_PREFIXES,
};
use anyhow::Error;
use serde::ser::SerializeStruct;
use serde::{
    de::{self, MapAccess, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

/// A map of string key–value pairs used for context variables in agent interactions.
pub type ContextVariables = HashMap<String, String>;

/// Represents instructions that can be given to an agent.
///
/// Instructions can be either static text or a dynamic function that generates
/// instructions based on context variables.
#[derive(Clone)]
pub enum Instructions {
    Text(String),
    Function(Arc<dyn Fn(ContextVariables) -> String + Send + Sync>),
}

/// Represents an AI agent with its configuration and capabilities.
///
/// An agent is defined by its name, model, instructions, and available functions.
#[derive(Clone)]
pub struct Agent {
    pub name: String,
    pub model: String,
    pub instructions: Instructions,
    pub functions: Vec<AgentFunction>,
    pub function_call: Option<String>,
    pub parallel_tool_calls: bool,
}

// Custom Debug implementation for Agent.
impl fmt::Debug for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Since we cannot print the functions (they may contain closures), we omit them.
        write!(
            f,
            "Agent {{ name: {}, model: {}, function_call: {:?}, parallel_tool_calls: {} }}",
            self.name, self.model, self.function_call, self.parallel_tool_calls
        )
    }
}

// Custom Serialize implementation for Agent.
impl Serialize for Agent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Agent", 5)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("model", &self.model)?;
        // We cannot serialize `instructions` or `functions` since they may contain closures.
        state.serialize_field("function_call", &self.function_call)?;
        state.serialize_field("parallel_tool_calls", &self.parallel_tool_calls)?;
        state.end()
    }
}

// Custom Deserialize implementation for Agent.
impl<'de> Deserialize<'de> for Agent {
    fn deserialize<D>(deserializer: D) -> Result<Agent, D::Error>
    where
        D: Deserializer<'de>,
    {
        enum Field {
            Name,
            Model,
            FunctionCall,
            ParallelToolCalls,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Field, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter
                            .write_str("`name`, `model`, `function_call`, or `parallel_tool_calls`")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            "name" => Ok(Field::Name),
                            "model" => Ok(Field::Model),
                            "function_call" => Ok(Field::FunctionCall),
                            "parallel_tool_calls" => Ok(Field::ParallelToolCalls),
                            _ => Err(de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct AgentVisitor;

        impl<'de> Visitor<'de> for AgentVisitor {
            type Value = Agent;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct Agent")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Agent, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut name = None;
                let mut model = None;
                let mut function_call = None;
                let mut parallel_tool_calls = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Name => {
                            if name.is_some() {
                                return Err(de::Error::duplicate_field("name"));
                            }
                            name = Some(map.next_value()?);
                        }
                        Field::Model => {
                            if model.is_some() {
                                return Err(de::Error::duplicate_field("model"));
                            }
                            model = Some(map.next_value()?);
                        }
                        Field::FunctionCall => {
                            if function_call.is_some() {
                                return Err(de::Error::duplicate_field("function_call"));
                            }
                            function_call = Some(map.next_value()?);
                        }
                        Field::ParallelToolCalls => {
                            if parallel_tool_calls.is_some() {
                                return Err(de::Error::duplicate_field("parallel_tool_calls"));
                            }
                            parallel_tool_calls = Some(map.next_value()?);
                        }
                    }
                }

                let name = name.ok_or_else(|| de::Error::missing_field("name"))?;
                let model = model.ok_or_else(|| de::Error::missing_field("model"))?;
                let parallel_tool_calls = parallel_tool_calls.unwrap_or(false);

                Ok(Agent {
                    name,
                    model,
                    instructions: Instructions::Text(String::new()), // Default value.
                    functions: Vec::new(),                           // Default value.
                    function_call,
                    parallel_tool_calls,
                })
            }
        }

        const FIELDS: &'static [&'static str] =
            &["name", "model", "function_call", "parallel_tool_calls"];

        deserializer.deserialize_struct("Agent", FIELDS, AgentVisitor)
    }
}

/// The result of an agent function execution.
#[derive(Clone, Debug)]
pub enum ResultType {
    Value(String),
    Agent(Agent),
    ContextVariables(ContextVariables),
}

impl ResultType {
    pub fn get_value(&self) -> String {
        match self {
            ResultType::Value(v) => v.clone(),
            _ => String::new(),
        }
    }

    pub fn get_agent(&self) -> Option<Agent> {
        if let ResultType::Agent(agent) = self {
            Some(agent.clone())
        } else {
            None
        }
    }

    pub fn get_context_variables(&self) -> ContextVariables {
        if let ResultType::ContextVariables(vars) = self {
            vars.clone()
        } else {
            HashMap::new()
        }
    }
}

/// Represents an asynchronous agent function.
///
/// The function field now returns a pinned future that will output a
/// `Result<ResultType, anyhow::Error>` when awaited.
#[derive(Clone)]
pub struct AgentFunction {
    pub name: String,
    pub function: Arc<
        dyn Fn(ContextVariables) -> Pin<Box<dyn Future<Output = Result<ResultType, Error>> + Send>>
            + Send
            + Sync,
    >,
    pub accepts_context_variables: bool,
}

/// Configuration settings for the Swarm instance.
#[derive(Clone, Debug)]
pub struct SwarmConfig {
    pub api_url: String,
    pub api_version: String,
    pub request_timeout: u64,
    pub connect_timeout: u64,
    pub max_retries: u32,
    pub max_loop_iterations: u32,
    pub valid_model_prefixes: Vec<String>,
    pub valid_api_url_prefixes: Vec<String>,
    pub loop_control: LoopControl,
    pub api_settings: ApiSettings,
}

/// Controls the execution of loops in agent interactions.
#[derive(Clone, Debug)]
pub struct LoopControl {
    pub default_max_iterations: u32,
    pub iteration_delay: Duration,
    pub break_conditions: Vec<String>,
}

impl Default for LoopControl {
    fn default() -> Self {
        LoopControl {
            default_max_iterations: 10,
            iteration_delay: Duration::from_millis(100),
            break_conditions: vec!["end_loop".to_string()],
        }
    }
}

/// API related settings for request handling.
#[derive(Clone, Debug)]
pub struct ApiSettings {
    pub retry_strategy: RetryStrategy,
    pub timeout_settings: TimeoutSettings,
}

impl Default for ApiSettings {
    fn default() -> Self {
        ApiSettings {
            retry_strategy: RetryStrategy {
                max_retries: 3,
                initial_delay: Duration::from_secs(1),
                max_delay: Duration::from_secs(30),
                backoff_factor: 2.0,
            },
            timeout_settings: TimeoutSettings {
                request_timeout: Duration::from_secs(30),
                connect_timeout: Duration::from_secs(10),
                read_timeout: Duration::from_secs(30),
                write_timeout: Duration::from_secs(30),
            },
        }
    }
}

impl Default for SwarmConfig {
    fn default() -> Self {
        SwarmConfig {
            api_url: OPENAI_DEFAULT_API_URL.to_string(),
            api_version: DEFAULT_API_VERSION.to_string(),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            max_retries: 3,
            max_loop_iterations: 10,
            valid_model_prefixes: vec![
                "gpt-".to_string(),
                "deepseek-".to_string(),
                "claude-".to_string(),
                "openai-".to_string(),
                "openrouter-".to_string(),
            ],
            valid_api_url_prefixes: VALID_API_URL_PREFIXES
                .iter()
                .map(|&s| s.to_string())
                .collect(),
            loop_control: LoopControl::default(),
            api_settings: ApiSettings::default(),
        }
    }
}

/// Represents a chat message.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    pub role: String,
    pub content: Option<String>,
    pub name: Option<String>,
    pub function_call: Option<FunctionCall>,
}

/// Represents a function call inside a message.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// The response from a chat completion request.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

/// A choice returned from the chat completion response.
///
/// This custom deserializer checks for the presence of a "message" field and,
/// if absent, falls back to a "delta" field.
#[derive(Serialize, Clone, Debug)]
pub struct Choice {
    pub index: u32,
    pub message: Message,
    pub finish_reason: Option<String>,
}

impl<'de> Deserialize<'de> for Choice {
    fn deserialize<D>(deserializer: D) -> Result<Choice, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        let index = value
            .get("index")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| de::Error::missing_field("index"))? as u32;

        let finish_reason = value
            .get("finish_reason")
            .and_then(|v| v.as_str())
            .map(String::from);

        let message = if let Some(msg_val) = value.get("message") {
            serde_json::from_value(msg_val.clone()).map_err(de::Error::custom)?
        } else if let Some(delta_val) = value.get("delta") {
            let role = delta_val
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let content = delta_val
                .get("content")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Message {
                role,
                content,
                name: None,
                function_call: None,
            }
        } else {
            return Err(de::Error::missing_field("message (or delta)"));
        };

        Ok(Choice {
            index,
            message,
            finish_reason,
        })
    }
}

/// Token usage metrics for a chat completion.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Represents a complete chat response.
#[derive(Clone, Debug)]
pub struct Response {
    pub messages: Vec<Message>,
    pub agent: Option<Agent>,
    pub context_variables: ContextVariables,
}

/// Represents a collection of steps parsed from XML.
#[derive(Debug, Deserialize)]
pub struct Steps {
    #[serde(rename = "step", default)]
    pub steps: Vec<Step>,
}

/// A single step in a steps definition.
#[derive(Debug, Deserialize)]
pub struct Step {
    #[serde(rename = "@number")]
    pub number: usize,
    #[serde(rename = "@action")]
    pub action: String,
    #[serde(rename = "@agent")]
    pub agent: Option<String>,
    pub prompt: String,
}

/// Strategy used for retrying failed API calls.
#[derive(Clone, Debug)]
pub struct RetryStrategy {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_factor: f32,
}

/// Timeout settings used for API calls.
#[derive(Clone, Debug)]
pub struct TimeoutSettings {
    pub request_timeout: Duration,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
}

/// Represents an error response from OpenAI.
#[derive(Debug, Deserialize)]
pub struct OpenAIErrorResponse {
    pub error: OpenAIError,
}

/// The detailed OpenAI error.
#[derive(Debug, Deserialize)]
pub struct OpenAIError {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
    pub param: Option<String>,
    pub code: Option<String>,
}
