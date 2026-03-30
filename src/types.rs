// File: rswarm/src/types.rs

use crate::constants::{
    DEFAULT_API_VERSION, DEFAULT_CONNECT_TIMEOUT, DEFAULT_MAX_LOOP_ITERATIONS,
    DEFAULT_REQUEST_TIMEOUT, OPENAI_DEFAULT_API_URL, VALID_API_URL_PREFIXES,
};
use crate::error::{SwarmError, SwarmResult};
use crate::phase::TerminationReason;
use anyhow::Error;
use serde_json::Value;
use serde::{
    de::{self},
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

/// A map of string key–value pairs used for context variables in agent interactions.
pub type ContextVariables = HashMap<String, String>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(value: impl Into<String>) -> SwarmResult<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "API key cannot be empty".to_string(),
            ));
        }
        if !value.starts_with("sk-") {
            return Err(SwarmError::ValidationError(
                "Invalid API key format".to_string(),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn redacted(&self) -> String {
        if self.0.len() <= 7 {
            "sk-***".to_string()
        } else {
            let suffix = &self.0[self.0.len() - 4..];
            format!("sk-***{}", suffix)
        }
    }
}

impl AsRef<str> for ApiKey {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.redacted())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ApiUrlPrefix(String);

impl ApiUrlPrefix {
    pub fn new(value: impl Into<String>) -> SwarmResult<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "API URL prefix cannot be empty".to_string(),
            ));
        }
        Ok(Self(value))
    }

    pub fn matches(&self, url: &str) -> bool {
        url.starts_with(&self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ApiUrlPrefix {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ApiUrl(String);

impl ApiUrl {
    pub fn new(value: impl Into<String>, allowed_prefixes: &[ApiUrlPrefix]) -> SwarmResult<Self> {
        let value = value.into();

        if value.trim().is_empty() {
            return Err(SwarmError::ValidationError("API URL cannot be empty".to_string()));
        }

        let parsed = Url::parse(&value).map_err(|e| {
            SwarmError::ValidationError(format!("Invalid API URL format: {}", e))
        })?;

        let host = parsed.host_str();
        let is_localhost = matches!(host, Some("localhost") | Some("127.0.0.1"));

        if !is_localhost && !value.starts_with("https://") {
            return Err(SwarmError::ValidationError(
                "API URL must start with https:// (except for localhost)".to_string(),
            ));
        }

        if !is_localhost && !allowed_prefixes.iter().any(|prefix| prefix.matches(&value)) {
            return Err(SwarmError::ValidationError(format!(
                "API URL must start with one of: {}",
                allowed_prefixes
                    .iter()
                    .map(ApiUrlPrefix::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ApiUrl {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ApiUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ModelPrefix(String);

impl ModelPrefix {
    pub fn new(value: impl Into<String>) -> SwarmResult<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "Model prefix cannot be empty".to_string(),
            ));
        }
        Ok(Self(value))
    }

    pub fn matches(&self, model: &str) -> bool {
        model.starts_with(&self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ModelPrefix {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ModelId(String);

impl ModelId {
    pub fn new(value: impl Into<String>, prefixes: &[ModelPrefix]) -> SwarmResult<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "Agent model cannot be empty".to_string(),
            ));
        }
        if !prefixes.iter().any(|prefix| prefix.matches(&value)) {
            return Err(SwarmError::ValidationError(format!(
                "Invalid model prefix. Model must start with one of: {:?}",
                prefixes
                    .iter()
                    .map(ModelPrefix::as_str)
                    .collect::<Vec<_>>()
            )));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ModelId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RequestTimeoutSeconds(u64);

impl RequestTimeoutSeconds {
    pub fn new(value: u64) -> SwarmResult<Self> {
        if value == 0 {
            return Err(SwarmError::ValidationError(
                "request_timeout must be greater than 0".to_string(),
            ));
        }
        if value < crate::constants::MIN_REQUEST_TIMEOUT
            || value > crate::constants::MAX_REQUEST_TIMEOUT
        {
            return Err(SwarmError::ValidationError(format!(
                "request_timeout must be between {} and {} seconds",
                crate::constants::MIN_REQUEST_TIMEOUT,
                crate::constants::MAX_REQUEST_TIMEOUT
            )));
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectTimeoutSeconds(u64);

impl ConnectTimeoutSeconds {
    pub fn new(value: u64) -> SwarmResult<Self> {
        if value == 0 {
            return Err(SwarmError::ValidationError(
                "connect_timeout must be greater than 0".to_string(),
            ));
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RetryLimit(u32);

impl RetryLimit {
    pub fn new(value: u32) -> SwarmResult<Self> {
        if value == 0 {
            return Err(SwarmError::ValidationError(
                "max_retries must be greater than 0".to_string(),
            ));
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LoopIterationLimit(u32);

impl LoopIterationLimit {
    pub fn new(value: u32) -> SwarmResult<Self> {
        if value == 0 {
            return Err(SwarmError::ValidationError(
                "default_max_iterations must be greater than 0".to_string(),
            ));
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FunctionCallPolicy {
    Disabled,
    Auto,
    Named(String),
}

impl FunctionCallPolicy {
    pub fn to_wire_value(&self) -> Option<String> {
        match self {
            Self::Disabled => None,
            Self::Auto => Some("auto".to_string()),
            Self::Named(name) => Some(name.clone()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallExecution {
    Serial,
    Parallel,
}

impl ToolCallExecution {
    pub fn is_parallel(self) -> bool {
        matches!(self, Self::Parallel)
    }
}

#[derive(Clone)]
pub struct Agent {
    pub(crate) name: String,
    pub(crate) model: String,
    pub(crate) instructions: Instructions,
    pub(crate) functions: Vec<AgentFunction>,
    pub(crate) function_call: FunctionCallPolicy,
    pub(crate) parallel_tool_calls: ToolCallExecution,
}

// Custom Debug implementation for Agent.
impl fmt::Debug for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Since we cannot print the functions (they may contain closures), we omit them.
        write!(
            f,
            "Agent {{ name: {}, model: {}, function_call: {:?}, parallel_tool_calls: {} }}",
            self.name,
            self.model,
            self.function_call,
            self.parallel_tool_calls.is_parallel()
        )
    }
}

impl Agent {
    pub fn new(
        name: impl Into<String>,
        model: impl Into<String>,
        instructions: Instructions,
    ) -> SwarmResult<Self> {
        let agent = Self {
            name: name.into(),
            model: model.into(),
            instructions,
            functions: Vec::new(),
            function_call: FunctionCallPolicy::Disabled,
            parallel_tool_calls: ToolCallExecution::Serial,
        };
        agent.validate_intrinsic_fields()?;
        Ok(agent)
    }

    pub fn with_functions(mut self, functions: Vec<AgentFunction>) -> Self {
        self.functions = functions;
        self
    }

    pub fn with_function_call_policy(mut self, function_call: FunctionCallPolicy) -> Self {
        self.function_call = function_call;
        self
    }

    pub fn with_tool_call_execution(mut self, parallel_tool_calls: ToolCallExecution) -> Self {
        self.parallel_tool_calls = parallel_tool_calls;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn instructions(&self) -> &Instructions {
        &self.instructions
    }

    pub fn functions(&self) -> &[AgentFunction] {
        &self.functions
    }

    pub fn function_call(&self) -> &FunctionCallPolicy {
        &self.function_call
    }

    pub fn tool_call_execution(&self) -> ToolCallExecution {
        self.parallel_tool_calls
    }

    pub(crate) fn validate_intrinsic_fields(&self) -> SwarmResult<()> {
        if self.name.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "Agent name cannot be empty".to_string(),
            ));
        }
        if self.model.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "Agent model cannot be empty".to_string(),
            ));
        }
        match &self.instructions {
            Instructions::Text(text) if text.trim().is_empty() => {
                return Err(SwarmError::ValidationError(
                    "Agent instructions cannot be empty".to_string(),
                ));
            }
            Instructions::Function(_) => {}
            _ => {}
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct AgentTransport {
    name: String,
    model: String,
    instructions: AgentInstructionsTransport,
    #[serde(default)]
    functions: Vec<AgentFunctionTransport>,
    #[serde(default)]
    function_call: Option<String>,
    #[serde(default)]
    parallel_tool_calls: bool,
}

#[derive(Serialize, Deserialize)]
struct AgentInstructionsTransport {
    text: String,
}

#[derive(Serialize, Deserialize)]
struct AgentFunctionTransport {
    name: String,
    accepts_context_variables: bool,
}

impl TryFrom<AgentTransport> for Agent {
    type Error = SwarmError;

    fn try_from(value: AgentTransport) -> Result<Self, Self::Error> {
        if !value.functions.is_empty() {
            return Err(SwarmError::ValidationError(
                "Agent deserialization does not support runtime function closures".to_string(),
            ));
        }

        let function_call = match value.function_call {
            None => FunctionCallPolicy::Disabled,
            Some(policy) if policy == "auto" => FunctionCallPolicy::Auto,
            Some(policy) if policy.trim().is_empty() => {
                return Err(SwarmError::ValidationError(
                    "Agent function_call policy cannot be empty".to_string(),
                ));
            }
            Some(policy) => FunctionCallPolicy::Named(policy),
        };

        Ok(Agent::new(
            value.name,
            value.model,
            Instructions::Text(value.instructions.text),
        )?
        .with_function_call_policy(function_call)
        .with_tool_call_execution(if value.parallel_tool_calls {
            ToolCallExecution::Parallel
        } else {
            ToolCallExecution::Serial
        }))
    }
}

impl Serialize for Agent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if !self.functions.is_empty() {
            return Err(serde::ser::Error::custom(
                "Agent serialization does not support runtime function closures",
            ));
        }

        let instructions = match &self.instructions {
            Instructions::Text(text) => AgentInstructionsTransport { text: text.clone() },
            Instructions::Function(_) => {
                return Err(serde::ser::Error::custom(
                    "Agent serialization does not support function-based instructions",
                ))
            }
        };

        AgentTransport {
            name: self.name.clone(),
            model: self.model.clone(),
            instructions,
            functions: Vec::new(),
            function_call: self.function_call.to_wire_value(),
            parallel_tool_calls: self.parallel_tool_calls.is_parallel(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Agent {
    fn deserialize<D>(deserializer: D) -> Result<Agent, D::Error>
    where
        D: Deserializer<'de>,
    {
        let dto = AgentTransport::deserialize(deserializer)?;
        Agent::try_from(dto).map_err(de::Error::custom)
    }
}

/// The result of an agent function execution.
#[derive(Clone, Debug)]
pub enum ResultType {
    Value(String),
    Agent(Agent),
    ContextVariables(ContextVariables),
    Termination(TerminationReason),
}

impl ResultType {
    pub fn into_value(self) -> Option<String> {
        match self {
            ResultType::Value(value) => Some(value),
            _ => None,
        }
    }

    pub fn into_agent(self) -> Option<Agent> {
        if let ResultType::Agent(agent) = self {
            Some(agent)
        } else {
            None
        }
    }

    pub fn into_context_variables(self) -> Option<ContextVariables> {
        if let ResultType::ContextVariables(vars) = self {
            Some(vars)
        } else {
            None
        }
    }

    pub fn into_termination_reason(self) -> Option<TerminationReason> {
        if let ResultType::Termination(reason) = self {
            Some(reason)
        } else {
            None
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
    api_url: ApiUrl,
    api_version: String,
    request_timeout: RequestTimeoutSeconds,
    connect_timeout: ConnectTimeoutSeconds,
    max_retries: RetryLimit,
    max_loop_iterations: LoopIterationLimit,
    valid_model_prefixes: Vec<ModelPrefix>,
    valid_api_url_prefixes: Vec<ApiUrlPrefix>,
    loop_control: LoopControl,
    api_settings: ApiSettings,
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
        let valid_model_prefixes = vec!["gpt-", "deepseek-", "claude-", "openai-", "openrouter-"]
            .into_iter()
            .map(|prefix| ModelPrefix::new(prefix).expect("default model prefixes are valid"))
            .collect();
        let valid_api_url_prefixes = VALID_API_URL_PREFIXES
            .iter()
            .map(|&prefix| ApiUrlPrefix::new(prefix).expect("default API URL prefixes are valid"))
            .collect::<Vec<_>>();

        SwarmConfig {
            api_url: ApiUrl::new(OPENAI_DEFAULT_API_URL.to_string(), &valid_api_url_prefixes)
                .expect("default API URL is valid"),
            api_version: DEFAULT_API_VERSION.to_string(),
            request_timeout: RequestTimeoutSeconds::new(DEFAULT_REQUEST_TIMEOUT)
                .expect("default request timeout is valid"),
            connect_timeout: ConnectTimeoutSeconds::new(DEFAULT_CONNECT_TIMEOUT)
                .expect("default connect timeout is valid"),
            max_retries: RetryLimit::new(3).expect("default retry limit is valid"),
            max_loop_iterations: LoopIterationLimit::new(DEFAULT_MAX_LOOP_ITERATIONS)
                .expect("default loop iteration limit is valid"),
            valid_model_prefixes,
            valid_api_url_prefixes,
            loop_control: LoopControl::default(),
            api_settings: ApiSettings::default(),
        }
    }
}

impl SwarmConfig {
    pub fn api_url(&self) -> &str {
        self.api_url.as_str()
    }

    pub fn api_version(&self) -> &str {
        &self.api_version
    }

    pub fn request_timeout(&self) -> u64 {
        self.request_timeout.get()
    }

    pub fn connect_timeout(&self) -> u64 {
        self.connect_timeout.get()
    }

    pub fn max_retries(&self) -> u32 {
        self.max_retries.get()
    }

    pub fn max_loop_iterations(&self) -> u32 {
        self.max_loop_iterations.get()
    }

    pub fn valid_model_prefixes(&self) -> &[ModelPrefix] {
        &self.valid_model_prefixes
    }

    pub fn valid_api_url_prefixes(&self) -> &[ApiUrlPrefix] {
        &self.valid_api_url_prefixes
    }

    pub fn loop_control(&self) -> &LoopControl {
        &self.loop_control
    }

    pub fn api_settings(&self) -> &ApiSettings {
        &self.api_settings
    }

    pub(crate) fn set_api_url(&mut self, api_url: impl Into<String>) -> SwarmResult<()> {
        self.api_url = ApiUrl::new(api_url, &self.valid_api_url_prefixes)?;
        Ok(())
    }

    pub(crate) fn set_api_version(&mut self, api_version: impl Into<String>) -> SwarmResult<()> {
        let api_version = api_version.into();
        if api_version.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "API version cannot be empty".to_string(),
            ));
        }
        self.api_version = api_version;
        Ok(())
    }

    pub(crate) fn set_request_timeout(&mut self, request_timeout: u64) -> SwarmResult<()> {
        let request_timeout = RequestTimeoutSeconds::new(request_timeout)?;
        self.request_timeout = request_timeout;
        self.api_settings.timeout_settings.request_timeout =
            Duration::from_secs(request_timeout.get());
        Ok(())
    }

    pub(crate) fn set_connect_timeout(&mut self, connect_timeout: u64) -> SwarmResult<()> {
        let connect_timeout = ConnectTimeoutSeconds::new(connect_timeout)?;
        self.connect_timeout = connect_timeout;
        self.api_settings.timeout_settings.connect_timeout =
            Duration::from_secs(connect_timeout.get());
        Ok(())
    }

    pub(crate) fn set_max_retries(&mut self, max_retries: u32) -> SwarmResult<()> {
        self.max_retries = RetryLimit::new(max_retries)?;
        self.api_settings.retry_strategy.max_retries = max_retries;
        Ok(())
    }

    pub(crate) fn set_max_loop_iterations(&mut self, max_loop_iterations: u32) -> SwarmResult<()> {
        let max_loop_iterations = LoopIterationLimit::new(max_loop_iterations)?;
        self.max_loop_iterations = max_loop_iterations;
        self.loop_control.default_max_iterations = max_loop_iterations.get();
        Ok(())
    }

    pub(crate) fn set_valid_model_prefixes(
        &mut self,
        valid_model_prefixes: Vec<String>,
    ) -> SwarmResult<()> {
        if valid_model_prefixes.is_empty() {
            return Err(SwarmError::ValidationError(
                "valid_model_prefixes cannot be empty".to_string(),
            ));
        }

        self.valid_model_prefixes = valid_model_prefixes
            .into_iter()
            .map(ModelPrefix::new)
            .collect::<SwarmResult<Vec<_>>>()?;
        Ok(())
    }

    pub(crate) fn set_valid_api_url_prefixes(
        &mut self,
        valid_api_url_prefixes: Vec<String>,
    ) -> SwarmResult<()> {
        if valid_api_url_prefixes.is_empty() {
            return Err(SwarmError::ValidationError(
                "valid_api_url_prefixes cannot be empty".to_string(),
            ));
        }

        let valid_api_url_prefixes = valid_api_url_prefixes
            .into_iter()
            .map(ApiUrlPrefix::new)
            .collect::<SwarmResult<Vec<_>>>()?;
        let current_api_url = self.api_url.as_str().to_string();

        self.valid_api_url_prefixes = valid_api_url_prefixes;
        self.api_url = ApiUrl::new(current_api_url, &self.valid_api_url_prefixes)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Function,
}

impl MessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Function => "function",
        }
    }
}

impl fmt::Display for MessageRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct FunctionCall {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct FunctionCallDto {
    name: String,
    arguments: String,
}

impl FunctionCall {
    pub fn new(name: impl Into<String>, arguments: impl Into<String>) -> SwarmResult<Self> {
        let function_call = Self {
            name: name.into(),
            arguments: arguments.into(),
        };
        function_call.validate()?;
        Ok(function_call)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn arguments(&self) -> &str {
        &self.arguments
    }

    pub(crate) fn from_parts_unchecked(name: String, arguments: String) -> Self {
        Self { name, arguments }
    }

    pub(crate) fn merge_delta(&mut self, delta: &Value) {
        if let Some(name) = delta.get("name").and_then(|value| value.as_str()) {
            self.name.push_str(name);
        }
        if let Some(arguments) = delta.get("arguments").and_then(|value| value.as_str()) {
            self.arguments.push_str(arguments);
        }
    }

    fn validate(&self) -> SwarmResult<()> {
        if self.name.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "Function call name cannot be empty".to_string(),
            ));
        }
        if self.arguments.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "Function call arguments cannot be empty".to_string(),
            ));
        }
        serde_json::from_str::<Value>(&self.arguments).map_err(|error| {
            SwarmError::ValidationError(format!(
                "Function call arguments must be valid JSON: {}",
                error
            ))
        })?;
        Ok(())
    }
}

impl<'de> Deserialize<'de> for FunctionCall {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let dto = FunctionCallDto::deserialize(deserializer)?;
        Self::new(dto.name, dto.arguments).map_err(de::Error::custom)
    }
}

/// Represents a chat message.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Message {
    role: MessageRole,
    content: Option<String>,
    name: Option<String>,
    function_call: Option<FunctionCall>,
}

#[derive(Deserialize)]
struct MessageDto {
    role: MessageRole,
    content: Option<String>,
    name: Option<String>,
    function_call: Option<FunctionCall>,
}

impl Message {
    pub fn new(
        role: MessageRole,
        content: Option<String>,
        name: Option<String>,
        function_call: Option<FunctionCall>,
    ) -> SwarmResult<Self> {
        let message = Self {
            role,
            content,
            name,
            function_call,
        };
        message.validate()?;
        Ok(message)
    }

    pub fn system(content: impl Into<String>) -> SwarmResult<Self> {
        Self::new(MessageRole::System, Some(content.into()), None, None)
    }

    pub fn user(content: impl Into<String>) -> SwarmResult<Self> {
        Self::new(MessageRole::User, Some(content.into()), None, None)
    }

    pub fn assistant(content: impl Into<String>) -> SwarmResult<Self> {
        Self::new(MessageRole::Assistant, Some(content.into()), None, None)
    }

    pub fn assistant_named(
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> SwarmResult<Self> {
        Self::new(
            MessageRole::Assistant,
            Some(content.into()),
            Some(name.into()),
            None,
        )
    }

    pub fn assistant_function_call(function_call: FunctionCall) -> SwarmResult<Self> {
        Self::new(MessageRole::Assistant, None, None, Some(function_call))
    }

    pub fn function(name: impl Into<String>, content: impl Into<String>) -> SwarmResult<Self> {
        Self::new(
            MessageRole::Function,
            Some(content.into()),
            Some(name.into()),
            None,
        )
    }

    pub fn role(&self) -> MessageRole {
        self.role
    }

    pub fn content(&self) -> Option<&str> {
        self.content.as_deref()
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn function_call(&self) -> Option<&FunctionCall> {
        self.function_call.as_ref()
    }

    pub fn validate(&self) -> SwarmResult<()> {
        if let Some(content) = &self.content {
            if content.trim().is_empty() {
                return Err(SwarmError::ValidationError(
                    "Message content cannot be empty".to_string(),
                ));
            }
        }
        if let Some(name) = &self.name {
            if name.trim().is_empty() {
                return Err(SwarmError::ValidationError(
                    "Message name cannot be empty".to_string(),
                ));
            }
        }

        match self.role {
            MessageRole::System | MessageRole::User => {
                if self.content.is_none() {
                    return Err(SwarmError::ValidationError(format!(
                        "{} messages require content",
                        self.role
                    )));
                }
                if self.name.is_some() {
                    return Err(SwarmError::ValidationError(format!(
                        "{} messages cannot set name",
                        self.role
                    )));
                }
                if self.function_call.is_some() {
                    return Err(SwarmError::ValidationError(format!(
                        "{} messages cannot include function calls",
                        self.role
                    )));
                }
            }
            MessageRole::Assistant => {
                let has_content = self.content.is_some();
                let has_function_call = self.function_call.is_some();
                if has_content == has_function_call {
                    return Err(SwarmError::ValidationError(
                        "Assistant messages must contain either content or a function call"
                            .to_string(),
                    ));
                }
                if has_function_call && self.name.is_some() {
                    return Err(SwarmError::ValidationError(
                        "Assistant function-call messages cannot set name".to_string(),
                    ));
                }
            }
            MessageRole::Function => {
                if self.content.is_none() {
                    return Err(SwarmError::ValidationError(
                        "Function messages require content".to_string(),
                    ));
                }
                if self.name.is_none() {
                    return Err(SwarmError::ValidationError(
                        "Function messages require a name".to_string(),
                    ));
                }
                if self.function_call.is_some() {
                    return Err(SwarmError::ValidationError(
                        "Function messages cannot include function calls".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn from_parts_unchecked(
        role: MessageRole,
        content: Option<String>,
        name: Option<String>,
        function_call: Option<FunctionCall>,
    ) -> Self {
        Self {
            role,
            content,
            name,
            function_call,
        }
    }

    pub(crate) fn append_content_fragment(&mut self, fragment: &str) {
        if fragment.is_empty() {
            return;
        }
        if let Some(existing_content) = &mut self.content {
            existing_content.push_str(fragment);
        } else {
            self.content = Some(fragment.to_string());
        }
    }

    pub(crate) fn merge_function_call_delta(&mut self, delta: &Value) {
        let function_call = self
            .function_call
            .get_or_insert_with(|| FunctionCall::from_parts_unchecked(String::new(), String::new()));
        function_call.merge_delta(delta);
    }
}

impl<'de> Deserialize<'de> for Message {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let dto = MessageDto::deserialize(deserializer)?;
        Self::new(dto.role, dto.content, dto.name, dto.function_call).map_err(de::Error::custom)
    }
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
                .cloned()
                .map(serde_json::from_value::<MessageRole>)
                .transpose()
                .map_err(de::Error::custom)?
                .unwrap_or(MessageRole::Assistant);
            let content = delta_val
                .get("content")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let mut message = Message::from_parts_unchecked(role, content, None, None);
            if let Some(function_call_delta) = delta_val.get("function_call") {
                message.merge_function_call_delta(function_call_delta);
            }
            message
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
    pub termination_reason: Option<TerminationReason>,
    pub tokens_used: u32,
}

/// Represents a collection of steps parsed from XML.
#[derive(Debug, Deserialize)]
pub struct Steps {
    #[serde(rename = "step", default)]
    pub steps: Vec<Step>,
}

/// A single step in a steps definition.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepAction {
    RunOnce,
    Loop,
}

impl fmt::Display for StepAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RunOnce => write!(f, "run_once"),
            Self::Loop => write!(f, "loop"),
        }
    }
}

/// A single step in a steps definition.
#[derive(Debug, Deserialize)]
pub struct Step {
    #[serde(rename = "@number")]
    pub number: usize,
    #[serde(rename = "@action")]
    pub action: StepAction,
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
