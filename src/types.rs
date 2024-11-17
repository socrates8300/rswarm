// #![allow(unused, dead_code)]  // TODO: Remove unused and dead code
// ./src/types.rs

use anyhow::Result;
use crate::constants::{OPENAI_DEFAULT_API_URL, DEFAULT_API_VERSION, DEFAULT_REQUEST_TIMEOUT, DEFAULT_CONNECT_TIMEOUT, VALID_API_URL_PREFIXES};
use serde::ser::SerializeStruct;
use serde::{
    de::{self, MapAccess, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

/// A map of string key-value pairs used for context variables in agent interactions
pub type ContextVariables = HashMap<String, String>;

/// Represents instructions that can be given to an agent
///
/// Instructions can be either static text or a dynamic function that generates
/// instructions based on context variables.
#[derive(Clone)]
pub enum Instructions {
    Text(String),
    Function(Arc<dyn Fn(ContextVariables) -> String + Send + Sync>),
}

/// Represents an AI agent with its configuration and capabilities
///
/// An agent is defined by its name, model, instructions, and available functions.
/// It can be configured to make function calls and handle parallel tool calls.
///
/// # Examples
///
/// ```rust
/// use rswarm::Agent;
///
/// let agent = Agent {
///     name: "assistant".to_string(),
///     model: "gpt-4".to_string(),
///     instructions: Instructions::Text("You are a helpful assistant.".to_string()),
///     functions: vec![],
///     function_call: None,
///     parallel_tool_calls: false,
/// };
/// ```
#[derive(Clone)]
pub struct Agent {
    pub name: String,
    pub model: String,
    pub instructions: Instructions,
    pub functions: Vec<AgentFunction>,
    pub function_call: Option<String>,
    pub parallel_tool_calls: bool,
}

// Custom Debug implementation for Agent
impl fmt::Debug for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Since we cannot print the functions, we can omit them from the Debug output
        write!(
            f,
            "Agent {{ name: {}, model: {}, function_call: {:?}, parallel_tool_calls: {} }}",
            self.name, self.model, self.function_call, self.parallel_tool_calls
        )
    }
}

// Custom Serialize implementation for Agent
impl Serialize for Agent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Agent", 5)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("model", &self.model)?;
        // We cannot serialize `instructions` or `functions` since they may contain functions
        // So we omit them
        state.serialize_field("function_call", &self.function_call)?;
        state.serialize_field("parallel_tool_calls", &self.parallel_tool_calls)?;
        state.end()
    }
}

// Custom Deserialize implementation for Agent
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
                    instructions: Instructions::Text(String::new()), // default value
                    functions: Vec::new(),                           // default value
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

/// Configuration settings for the Swarm instance
///
/// Contains all configuration parameters for API communication,
/// request handling, and execution control.
///
/// # Examples
///
/// ```rust
/// use rswarm::SwarmConfig;
///
/// let config = SwarmConfig {
///     api_url: "https://api.openai.com/v1".to_string(),
///     api_version: "v1".to_string(),
///     request_timeout: 30,
///     connect_timeout: 10,
///     max_retries: 3,
///     max_loop_iterations: 10,
///     valid_model_prefixes: vec!["gpt-".to_string()],
///     valid_api_url_prefixes: vec!["https://api.openai.com".to_string()],
///     loop_control: LoopControl::default(),
///     api_settings: ApiSettings::default(),
/// };
/// ```
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

/// Controls the execution of loops in agent interactions
///
/// Defines parameters for controlling iteration limits, delays between
/// iterations, and conditions for breaking loops.
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

/// API-related settings for request handling
///
/// Contains configurations for retry behavior and timeout settings
/// for various types of API operations.
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
            valid_model_prefixes: vec!["gpt-".to_string()],
            valid_api_url_prefixes: VALID_API_URL_PREFIXES
                .iter()
                .map(|&s| s.to_string())
                .collect(),
            loop_control: LoopControl::default(),
            api_settings: ApiSettings::default(),
        }
    }
}

/// Represents a function that can be called by an agent
///
/// Functions can accept context variables and return various types of results.
/// They are used to extend agent capabilities with custom functionality.
#[derive(Clone)]
pub struct AgentFunction {
    pub name: String,
    pub function: Arc<dyn Fn(ContextVariables) -> Result<ResultType> + Send + Sync>,
    pub accepts_context_variables: bool,
}

// Since we cannot serialize or debug function pointers, we need custom implementations or omit them

impl fmt::Debug for AgentFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Omit the function field in Debug output
        write!(
            f,
            "AgentFunction {{ name: {}, accepts_context_variables: {} }}",
            self.name, self.accepts_context_variables
        )
    }
}

/// Represents a message in a conversation
///
/// Messages can be from different roles (system, user, assistant, function)
/// and may include function calls.
///
/// # Examples
///
/// ```rust
/// use rswarm::Message;
///
/// let message = Message {
///     role: "user".to_string(),
///     content: Some("Hello, assistant!".to_string()),
///     name: None,
///     function_call: None,
/// };
/// ```
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    pub role: String,
    pub content: Option<String>,
    pub name: Option<String>,
    pub function_call: Option<FunctionCall>,
}

/// Represents a function call made by an agent
///
/// Contains the name of the function to call and its arguments as a JSON string.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Response from a chat completion API call
///
/// Contains the complete response from the API, including
/// message choices and usage statistics.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Choice {
    pub index: u32,
    pub message: Message,
    pub finish_reason: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Represents the possible types of results from function calls
///
/// Results can be string values, agents, or context variables.
#[derive(Clone, Debug)]
pub enum ResultType {
    /// A simple string value
    Value(String),
    /// An agent configuration
    Agent(Agent),
    /// A map of context variables
    ContextVariables(ContextVariables),
}

impl ResultType {
    /// Extracts the string value from the ResultType
    ///
    /// Returns an empty string if the ResultType is not a Value variant.
    pub fn get_value(&self) -> String {
        match self {
            ResultType::Value(v) => v.clone(),
            _ => String::new(),
        }
    }

    /// Extracts the Agent from the ResultType
    ///
    /// Returns None if the ResultType is not an Agent variant.
    pub fn get_agent(&self) -> Option<Agent> {
        if let ResultType::Agent(agent) = self {
            Some(agent.clone())
        } else {
            None
        }
    }

    /// Extracts the ContextVariables from the ResultType
    ///
    /// Returns an empty HashMap if the ResultType is not a ContextVariables variant.
    pub fn get_context_variables(&self) -> ContextVariables {
        if let ResultType::ContextVariables(vars) = self {
            vars.clone()
        } else {
            HashMap::new()
        }
    }
}


/// Response from an agent interaction
///
/// Contains the messages generated, the final agent state,
/// and any context variables produced during the interaction.
#[derive(Clone, Debug)]
pub struct Response {
    pub messages: Vec<Message>,
    pub agent: Option<Agent>,
    pub context_variables: ContextVariables,
}

#[derive(Debug, Deserialize)]
pub struct Steps {
    #[serde(rename = "step", default)]
    pub steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
pub struct Step {
    #[serde(rename = "@number")]
    pub number: usize,
    #[serde(rename = "@action")]
    pub action: String,
    #[serde(rename = "@agent")]
    pub agent: Option<String>, // New field for agent name
    pub prompt: String,
}

/// Represents retry strategy configuration
///
/// Defines parameters for handling retries of failed requests,
/// including delays and backoff factors.
#[derive(Clone, Debug)]
pub struct RetryStrategy {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_factor: f32,
}

/// Timeout settings for various types of operations
///
/// Defines timeout durations for different aspects of API communication.
#[derive(Clone, Debug)]
pub struct TimeoutSettings {
    pub request_timeout: Duration,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
}
