// #![allow(unused, dead_code)]  // TODO: Remove unused and dead code
// ./types.rs

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

pub type ContextVariables = HashMap<String, String>;

#[derive(Clone)]
pub enum Instructions {
    Text(String),
    Function(Arc<dyn Fn(ContextVariables) -> String + Send + Sync>),
}

// Custom implementations for Agent
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
        }
    }
}

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

// We can skip Serialize and Deserialize for AgentFunction if not needed

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    pub role: String,
    pub content: Option<String>,
    pub name: Option<String>,
    pub function_call: Option<FunctionCall>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

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
