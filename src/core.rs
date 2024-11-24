// rswarm/src/core.rs

use crate::constants::{
    CTX_VARS_NAME, OPENAI_DEFAULT_API_URL, ROLE_ASSISTANT, ROLE_FUNCTION, ROLE_SYSTEM,
    MIN_REQUEST_TIMEOUT, MAX_REQUEST_TIMEOUT,
};
use crate::types::{
    Agent, AgentFunction, ChatCompletionResponse, ContextVariables, FunctionCall, Instructions,
    Message, Response, ResultType, SwarmConfig,
};

use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::time::Duration;
#[cfg(test)]
#[allow(unused_imports)]
use std::sync::Arc;

use crate::types::{Step, Steps};
use crate::util::{debug_print, extract_xml_steps, function_to_json, parse_steps_from_xml};
use crate::error::{SwarmError, SwarmResult};
use crate::validation::validate_api_request;
use crate::validation::validate_api_url;

impl Default for Swarm {
    fn default() -> Self {
        Swarm::new(None, None, HashMap::new())
            .expect("Default initialization should never fail")
    }
}

/// Main struct for managing AI agent interactions and chat completions
///
/// The Swarm struct provides the core functionality for managing AI agents,
/// handling chat completions, and executing function calls. It maintains
/// a registry of agents and handles API communication with OpenAI.
///
/// # Examples
///
/// ```rust
/// use rswarm::Swarm;
///
/// let swarm = Swarm::builder()
///     .with_api_key("your-api-key".to_string())
///     .build()
///     .expect("Failed to create Swarm");
/// ```
pub struct Swarm {
    pub client: Client,
    pub api_key: String,
    pub agent_registry: HashMap<String, Agent>, //
    pub config: SwarmConfig,
}

/// Builder pattern implementation for creating Swarm instances
///
/// Provides a flexible way to configure and create a new Swarm instance
/// with custom settings and validations.
///
/// # Examples
///
/// ```rust
/// use rswarm::SwarmBuilder;
///
/// let swarm = SwarmBuilder::new()
///     .with_api_key("your-api-key".to_string())
///     .with_request_timeout(30)
///     .build()
///     .expect("Failed to build Swarm");
/// ```
pub struct SwarmBuilder {
    client: Option<Client>,
    api_key: Option<String>,
    agents: HashMap<String, Agent>,
    config: SwarmConfig,
}

impl SwarmBuilder {
    /// Creates a new SwarmBuilder instance with default configuration
    pub fn new() -> Self {
        let config = SwarmConfig::default();
        SwarmBuilder {
            client: None,
            api_key: None,
            agents: HashMap::new(),
            config,
        }
    }

    /// Sets a custom configuration for the Swarm
    ///
    /// # Arguments
    ///
    /// * `config` - A SwarmConfig instance containing custom configuration values
    pub fn with_config(mut self, config: SwarmConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_api_url(mut self, api_url: String) -> Self {
        self.config.api_url = api_url;
        self
    }

    pub fn with_api_version(mut self, version: String) -> Self {
        self.config.api_version = version;
        self
    }

    pub fn with_request_timeout(mut self, timeout: u64) -> Self {
        self.config.request_timeout = timeout;
        self
    }

    pub fn with_connect_timeout(mut self, timeout: u64) -> Self {
        self.config.connect_timeout = timeout;
        self
    }

    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.config.max_retries = retries;
        self
    }

    pub fn with_max_loop_iterations(mut self, iterations: u32) -> Self {
        self.config.max_loop_iterations = iterations;
        self
    }

    pub fn with_valid_model_prefixes(mut self, prefixes: Vec<String>) -> Self {
        self.config.valid_model_prefixes = prefixes;
        self
    }

    pub fn with_valid_api_url_prefixes(mut self, prefixes: Vec<String>) -> Self {
        self.config.valid_api_url_prefixes = prefixes;
        self
    }

    pub fn with_client(mut self, client: Client) -> Self {
        self.client = Some(client);
        self
    }

    pub fn with_api_key(mut self, api_key: String) -> Self {
        self.api_key = Some(api_key);
        self
    }

    pub fn with_agent(mut self, agent: Agent) -> Self {
        self.agents.insert(agent.name.clone(), agent);
        self
    }

    /// Builds the Swarm instance with the configured settings
    ///
    /// # Returns
    ///
    /// Returns a Result containing either the configured Swarm instance
    /// or a SwarmError if validation fails
    ///
    /// # Errors
    ///
    /// Will return an error if:
    /// - API key is missing or invalid
    /// - API URL validation fails
    /// - Configuration validation fails
    pub fn build(self) -> SwarmResult<Swarm> {
        // First validate the configuration
        self.config.validate()?;

        // Validate all agents
        for agent in self.agents.values() {
            agent.validate(&self.config)?;
        }

        // Get API key from builder or environment
        let api_key = match self.api_key.or_else(|| env::var("OPENAI_API_KEY").ok()) {
            Some(key) => key,
            None => return Err(SwarmError::ValidationError("API key must be set either in environment or passed to builder".to_string())),
        };

        // Validate API key
        if api_key.trim().is_empty() {
            return Err(SwarmError::ValidationError("API key cannot be empty".to_string()));
        }
        if !api_key.starts_with("sk-") {
            return Err(SwarmError::ValidationError("Invalid API key format".to_string()));
        }

        // Get and validate API URL
        let api_url = env::var("OPENAI_API_URL")
            .unwrap_or_else(|_| OPENAI_DEFAULT_API_URL.to_string());

        // Pass the config to validate_api_url
        validate_api_url(&api_url, &self.config)?;

        // Create client with timeout configuration
        let client = self.client.unwrap_or_else(|| {
            Client::builder()
                .timeout(Duration::from_secs(
                    self.config.request_timeout
                ))
                .connect_timeout(Duration::from_secs(
                    self.config.connect_timeout
                ))
                .build()
                .unwrap_or_else(|_| Client::new())
        });

        Ok(Swarm {
            client,
            api_key,
            agent_registry: self.agents,
            config: self.config
        })
    }

    // Add validation method
    fn _validate(&self) -> SwarmResult<()> {
        // Validate API key if present
        if let Some(ref key) = self.api_key {
            if key.trim().is_empty() || !key.starts_with("sk-") {
                return Err(SwarmError::ValidationError("Invalid API key format".to_string()));
            }
        }

        // Validate config
        self.config.validate()?;

        Ok(())
    }
}

impl Swarm {
    /// Creates a new SwarmBuilder instance
    ///
    /// This is the recommended way to create a new Swarm instance.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rswarm::Swarm;
    ///
    /// let swarm = Swarm::builder()
    ///     .with_api_key("your-api-key".to_string())
    ///     .build()
    ///     .expect("Failed to create Swarm");
    /// ```
    pub fn builder() -> SwarmBuilder {
        SwarmBuilder::new()
    }

    // Keep existing new() method for backward compatibility
    pub fn new(
        client: Option<Client>,
        api_key: Option<String>,
        _agents: HashMap<String, Agent>,
    ) -> SwarmResult<Self> {
        let mut builder = SwarmBuilder::new();

        if let Some(client) = client {
            builder = builder.with_client(client);
        }
        if let Some(api_key) = api_key {
            builder = builder.with_api_key(api_key);
        }

        builder.build()
    }

    /// Makes a chat completion request to the OpenAI API
    ///
    /// # Arguments
    ///
    /// * `agent` - The Agent configuration to use for the request
    /// * `history` - Vector of previous messages in the conversation
    /// * `context_variables` - Variables to be used in the conversation context
    /// * `model_override` - Optional model to use instead of the agent's default
    /// * `stream` - Whether to stream the response
    /// * `debug` - Whether to enable debug output
    ///
    /// # Returns
    ///
    /// Returns a Result containing either the ChatCompletionResponse or a SwarmError
    ///
    /// # Errors
    ///
    /// Will return an error if:
    /// - API key is invalid or empty
    /// - Message history is empty
    /// - Network request fails
    /// - Response parsing fails
    pub async fn get_chat_completion(
        &self,
        agent: &Agent,
        history: &[Message],
        context_variables: &ContextVariables,
        model_override: Option<String>,
        stream: bool,
        debug: bool,
    ) -> SwarmResult<ChatCompletionResponse> {
        // Validate inputs
        if self.api_key.is_empty() {
            return Err(SwarmError::ValidationError("API key cannot be empty".to_string()));
        }

        if history.is_empty() {
            return Err(SwarmError::ValidationError("Message history cannot be empty".to_string()));
        }

        let instructions = match &agent.instructions {
            Instructions::Text(text) => text.clone(),
            Instructions::Function(func) => func(context_variables.clone()),
        };

        let mut messages = vec![Message {
            role: ROLE_SYSTEM.to_string(),
            content: Some(instructions),
            name: None,
            function_call: None,
        }];

        messages.extend_from_slice(history);

        debug_print(
            debug,
            &format!("Getting chat completion for...: {:?}", messages),
        );

        // Convert agent functions to functions for the API
        let functions: Vec<Value> = agent.functions.iter()
            .map(function_to_json)
            .collect::<SwarmResult<Vec<Value>>>()?;

        let model = model_override.unwrap_or_else(|| agent.model.clone());

        let mut request_body = json!({
            "model": model,
            "messages": messages,
        });

        if !functions.is_empty() {
            request_body["functions"] = Value::Array(functions);
        }

        if let Some(function_call) = &agent.function_call {
            request_body["function_call"] = json!(function_call);
        }

        if stream {
            request_body["stream"] = json!(true);
        }

        let url = env::var("OPENAI_API_URL")
            .map(|url| {
                if !url.starts_with("https://") {
                    return Err(SwarmError::ValidationError("OPENAI_API_URL must start with https://".to_string()));
                }
                Ok(url)
            })
            .unwrap_or_else(|_| Ok(OPENAI_DEFAULT_API_URL.to_string()))?;

        // Make the API request to OpenAI
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| SwarmError::NetworkError(e.to_string()))?;

        if stream {
            // Handle streaming response
            let mut stream = response.bytes_stream();

            let mut full_response = ChatCompletionResponse {
                id: "".to_string(),
                object: "chat.completion".to_string(),
                created: 0,
                choices: Vec::new(),
                usage: None,
            };

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(data) => {
                        let text = String::from_utf8_lossy(&data);

                        for line in text.lines() {
                            if line.starts_with("data: ") {
                                let json_str = line[6..].trim();
                                if json_str == "[DONE]" {
                                    break;
                                }
                                let partial_response: serde_json::Result<ChatCompletionResponse> =
                                    serde_json::from_str(json_str);
                                if let Ok(partial) = partial_response {
                                    // Merge partial into full_response
                                    full_response.choices.extend(partial.choices);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        return Err(SwarmError::StreamError(format!("Error reading streaming response: {}", e)));
                    }
                }
            }

            Ok(full_response)
        } else {
            response.json::<ChatCompletionResponse>().await
                .map_err(|e| SwarmError::DeserializationError(e.to_string()))
        }
    }

    pub fn handle_function_result(&self, result: ResultType, debug: bool) -> SwarmResult<ResultType> {
        match result {
            ResultType::Value(_) | ResultType::Agent(_) => Ok(result),
            _ => {
                let error_message = format!(
                    "Failed to cast response to string: {:?}. \
                    Make sure agent functions return a string or ResultType object.",
                    result
                );
                debug_print(debug, &error_message);
                Err(SwarmError::FunctionError(error_message))
            }
        }
    }

    pub fn handle_function_call(
        &self,
        function_call: &FunctionCall,
        functions: &[AgentFunction],
        context_variables: &mut ContextVariables,
        debug: bool,
    ) -> SwarmResult<Response> {
        // Validate function_call.name
        if function_call.name.trim().is_empty() {
            return Err(SwarmError::ValidationError("Function call name cannot be empty.".to_string()));
        }

        let mut function_map = HashMap::new();
        for func in functions {
            function_map.insert(func.name.clone(), func.clone());
        }

        let mut response = Response {
            messages: Vec::new(),
            agent: None,
            context_variables: HashMap::new(),
        };

        let name = &function_call.name;
        if let Some(func) = function_map.get(name) {
            let args: ContextVariables = serde_json::from_str(&function_call.arguments)?;
            debug_print(
                debug,
                &format!(
                    "Processing function call: {} with arguments {:?}",
                    name, args
                ),
            );

            let mut args = args.clone();
            if func.accepts_context_variables {
                let serialized_context = serde_json::to_string(&context_variables)?;
                args.insert(CTX_VARS_NAME.to_string(), serialized_context);
            }

            let raw_result = (func.function)(args)?;

            let result = self.handle_function_result(raw_result, debug)?;

            response.messages.push(Message {
                role: ROLE_FUNCTION.to_string(),
                name: Some(name.clone()),
                content: Some(result.get_value()),
                function_call: None,
            });

            response
                .context_variables
                .extend(result.get_context_variables());
            if let Some(agent) = result.get_agent() {
                response.agent = Some(agent);
            }
        } else {
            debug_print(debug, &format!("Function {} not found.", name));
            response.messages.push(Message {
                role: ROLE_ASSISTANT.to_string(),
                name: Some(name.clone()),
                content: Some(format!("Error: Function {} not found.", name)),
                function_call: None,
            });
        }

        Ok(response)
    }

    async fn single_execution(
        &self,
        agent: &Agent,
        history: &mut Vec<Message>,
        context_variables: &mut ContextVariables,
        model_override: Option<String>,
        stream: bool,
        debug: bool,
    ) -> SwarmResult<Response> {
        let completion = self
            .get_chat_completion(
                agent,
                &history.clone(),
                context_variables,
                model_override.clone(),
                stream,
                debug,
            )
            .await?;

        if completion.choices.is_empty() {
            return Err(SwarmError::ApiError("No choices returned from the model".to_string()));
        }

        let choice = &completion.choices[0];
        let message = choice.message.clone();

        history.push(message.clone());

        // Handle function calls if any
        if let Some(function_call) = &message.function_call {
            let func_response = self.handle_function_call(
                function_call,
                &agent.functions,
                context_variables,
                debug,
            )?;

            history.extend(func_response.messages.clone());
            context_variables.extend(func_response.context_variables);

            // If the function returns a new agent, we need to handle it
            // (In this implementation, we are not changing the agent here)
        }

        Ok(Response {
            messages: vec![message],
            agent: Some(agent.clone()),
            context_variables: context_variables.clone(),
        })
    }

    async fn execute_step(
        &self,
        agent: &mut Agent,
        history: &mut Vec<Message>,
        context_variables: &mut ContextVariables,
        model_override: Option<String>,
        stream: bool,
        debug: bool,
        max_turns: usize,
        step_number: usize,
        step: &Step,
    ) -> SwarmResult<Response> {
        // Validate step
        if step.prompt.trim().is_empty() {
            return Err(SwarmError::ValidationError("Step prompt cannot be empty".to_string()));
        }

        if step.number == 0 {
            return Err(SwarmError::ValidationError("Step number must be greater than 0".to_string()));
        }

        println!("Executing Step {}", step_number);

        // Handle agent handoff if an agent is specified in the step
        if let Some(agent_name) = &step.agent {
            println!("Switching to agent: {}", agent_name);
            *agent = self.get_agent_by_name(agent_name)?;
        }

        match step.action.as_str() {
            "run_once" => {
                // Prepare a message with the step's prompt
                let step_message = Message {
                    role: "user".to_string(),
                    content: Some(step.prompt.clone()),
                    name: None,
                    function_call: None,
                };

                history.push(step_message);

                // Proceed with a single execution
                self.single_execution(
                    agent,
                    history,
                    context_variables,
                    model_override.clone(),
                    stream,
                    debug,
                )
                .await
            }
            "loop" => {
                let mut iteration_count = 0;
                let max_iterations = 10; // Define a suitable maximum

                loop {
                    iteration_count += 1;

                    // Prepare a message with the step's prompt
                    let step_message = Message {
                        role: "user".to_string(),
                        content: Some(step.prompt.clone()),
                        name: None,
                        function_call: None,
                    };

                    history.push(step_message);

                    let response = self
                        .single_execution(
                            agent,
                            history,
                            context_variables,
                            model_override.clone(),
                            stream,
                            debug,
                        )
                        .await?;

                    // Handle agent change within the response
                    if let Some(new_agent) = response.agent.clone() {
                        *agent = new_agent;
                    }

                    // Decide when to break the loop
                    if context_variables.get("end_loop") == Some(&"true".to_string()) {
                        println!("Loop termination condition met.");
                        break;
                    }

                    // Prevent infinite loops with a max iteration count
                    if iteration_count >= max_iterations {
                        println!("Reached maximum loop iterations.");
                        break;
                    }

                    // Optional: Add a condition to prevent exceeding max_turns
                    if history.len() >= max_turns {
                        println!("Max turns reached in loop, exiting.");
                        break;
                    }
                }
                Ok(Response {
                    messages: history.clone(),
                    agent: Some(agent.clone()),
                    context_variables: context_variables.clone(),
                })
            }
            _ => {
                println!("Unknown action: {}", step.action);
                Err(SwarmError::ValidationError(format!("Unknown action: {}", step.action)))
            }
        }
    }

    pub fn get_agent_by_name(&self, name: &str) -> SwarmResult<Agent> {
        self.agent_registry
            .get(name)
            .cloned()
            .ok_or_else(|| SwarmError::AgentNotFoundError(name.to_string()))
    }

    /// Executes a conversation with an AI agent
    ///
    /// # Arguments
    ///
    /// * `agent` - The Agent to use for the conversation
    /// * `messages` - Initial messages to start the conversation
    /// * `context_variables` - Variables to be used in the conversation
    /// * `model_override` - Optional model to use instead of the agent's default
    /// * `stream` - Whether to stream the response
    /// * `debug` - Whether to enable debug output
    /// * `max_turns` - Maximum number of conversation turns
    ///
    /// # Returns
    ///
    /// Returns a Result containing either the Response or a SwarmError
    ///
    /// # Errors
    ///
    /// Will return an error if:
    /// - Input validation fails
    /// - Max turns exceeds configuration limits
    /// - API requests fail
    pub async fn run(
        &self,
        mut agent: Agent,
        messages: Vec<Message>,
        mut context_variables: ContextVariables,
        model_override: Option<String>,
        stream: bool,
        debug: bool,
        max_turns: usize,
    ) -> SwarmResult<Response> {
        // Use config for validation
        validate_api_request(&agent, &messages, &model_override, max_turns)?;

        // Use config values for loop control
        if max_turns > self.config.max_loop_iterations as usize {
            return Err(SwarmError::ValidationError(
                format!("max_turns ({}) exceeds configured max_loop_iterations ({})",
                    max_turns, self.config.max_loop_iterations)
            ));
        }

        // Extract XML steps from the agent's instructions
        let instructions = match &agent.instructions {
            Instructions::Text(text) => text.clone(),
            Instructions::Function(func) => func(context_variables.clone()),
        };

        let (instructions_without_xml, xml_steps) = extract_xml_steps(&instructions)?;

        // Parse the steps from XML
        let steps = if let Some(xml_content) = xml_steps {
            parse_steps_from_xml(&xml_content)?
        } else {
            Steps { steps: Vec::new() }
        };

        // Set the agent's instructions without the XML steps
        agent.instructions = Instructions::Text(instructions_without_xml);

        // Prepare history
        let mut history = messages.clone();

        // If there are steps, execute them
        if !steps.steps.is_empty() {
            for step in &steps.steps {
                let response = self
                    .execute_step(
                        &mut agent,
                        &mut history,
                        &mut context_variables,
                        model_override.clone(),
                        stream,
                        debug,
                        max_turns,
                        step.number,
                        step,
                    )
                    .await?;

                // Handle agent change
                if let Some(new_agent) = response.agent {
                    agent = new_agent;
                }

                // Optionally handle context variables or messages after each step
            }
        } else {
            // No steps, proceed with a default execution if necessary
            println!("No steps defined. Executing default behavior.");

            let response = self
                .single_execution(
                    &agent,
                    &mut history,
                    &mut context_variables,
                    model_override,
                    stream,
                    debug,
                )
                .await?;

            history.extend(response.messages);
            context_variables.extend(response.context_variables);
        }

        Ok(Response {
            messages: history,
            agent: Some(agent),
            context_variables,
        })
    }
}

impl SwarmConfig {
    pub fn validate(&self) -> SwarmResult<()> {
        if self.request_timeout == 0 {
            return Err(SwarmError::ValidationError("request_timeout must be greater than 0".to_string()));
        }
        if self.connect_timeout == 0 {
            return Err(SwarmError::ValidationError("connect_timeout must be greater than 0".to_string()));
        }
        if self.max_retries == 0 {
            return Err(SwarmError::ValidationError("max_retries must be greater than 0".to_string()));
        }
        if self.valid_model_prefixes.is_empty() {
            return Err(SwarmError::ValidationError("valid_model_prefixes cannot be empty".to_string()));
        }
        if self.request_timeout < MIN_REQUEST_TIMEOUT || self.request_timeout > MAX_REQUEST_TIMEOUT {
            return Err(SwarmError::ValidationError(
                format!("request_timeout must be between {} and {} seconds",
                    MIN_REQUEST_TIMEOUT, MAX_REQUEST_TIMEOUT)
            ));
        }
        if self.loop_control.default_max_iterations == 0 {
            return Err(SwarmError::ValidationError(
                "default_max_iterations must be greater than 0".to_string()
            ));
        }

        // Validate API URL format
        if !self.api_url.starts_with("https://") {
            return Err(SwarmError::ValidationError("API URL must start with https://".to_string()));
        }

        Ok(())
    }
}

impl Agent {
    pub fn validate(&self, config: &SwarmConfig) -> SwarmResult<()> {
        // Validate name
        if self.name.trim().is_empty() {
            return Err(SwarmError::ValidationError("Agent name cannot be empty".to_string()));
        }

        // Validate model
        if self.model.trim().is_empty() {
            return Err(SwarmError::ValidationError("Agent model cannot be empty".to_string()));
        }

        // Validate model prefix
        if !config.valid_model_prefixes.iter().any(|prefix| self.model.starts_with(prefix)) {
            return Err(SwarmError::ValidationError(format!(
                "Invalid model prefix. Model must start with one of: {:?}",
                config.valid_model_prefixes
            )));
        }

        // Validate instructions
        match &self.instructions {
            Instructions::Text(text) if text.trim().is_empty() => {
                return Err(SwarmError::ValidationError("Agent instructions cannot be empty".to_string()));
            }
            Instructions::Function(_) => {} // Function instructions are validated at runtime
            _ => {}
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ApiSettings, Agent, Instructions};
    use std::sync::Arc;


    #[test]
    fn test_valid_swarm_initialization() {
        // Create custom config
        let config = SwarmConfig {
            api_url: "https://api.openai.com/v1".to_string(),
            api_version: "v1".to_string(),
            request_timeout: 30,
            connect_timeout: 10,
            api_settings: ApiSettings::default(),
            max_retries: 3,
            max_loop_iterations: 10,
            valid_model_prefixes: vec!["gpt-".to_string()],
            valid_api_url_prefixes: vec!["https://api.openai.com".to_string()],
            loop_control: Default::default(),
        };

        // Create test agent
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        // Initialize Swarm using builder pattern
        let swarm = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_config(config.clone())
            .with_agent(agent.clone())
            .build()
            .expect("Failed to create Swarm");

        // Verify fields are correctly set
        assert_eq!(swarm.api_key, "sk-test123456789");
        assert_eq!(swarm.config.api_url, config.api_url);
        assert_eq!(swarm.config.request_timeout, config.request_timeout);
        assert_eq!(swarm.config.connect_timeout, config.connect_timeout);
        assert_eq!(swarm.config.max_retries, config.max_retries);
        assert!(swarm.agent_registry.contains_key("test_agent"));
        assert_eq!(swarm.agent_registry["test_agent"].name, agent.name);
        assert_eq!(swarm.agent_registry["test_agent"].model, agent.model);
    }

    #[test]
    fn test_default_swarm_initialization() {
        // Test default initialization using environment variable
        std::env::set_var("OPENAI_API_KEY", "sk-test123456789");

        let swarm = Swarm::default();

        // Verify default values
        assert_eq!(swarm.api_key, "sk-test123456789");
        assert!(swarm.agent_registry.is_empty());
        assert_eq!(swarm.config.api_url, OPENAI_DEFAULT_API_URL);

        // Clean up
        std::env::remove_var("OPENAI_API_KEY");
    }

    #[test]
    fn test_missing_api_key() {
        // Remove API key from environment if present
        std::env::remove_var("OPENAI_API_KEY");

        // Attempt to create Swarm without API key
        let result = Swarm::builder().build();

        // Verify error
        assert!(result.is_err());
        match result {
            Err(SwarmError::ValidationError(msg)) => {
                assert!(msg.contains("API key must be set"));
            }
            _ => panic!("Expected ValidationError for missing API key"),
        }
    }

    #[test]
    fn test_invalid_configurations() {
        // Test cases with invalid configurations
        let test_cases = vec![
            (
                SwarmConfig {
                    request_timeout: 0,
                    ..SwarmConfig::default()
                },
                "request_timeout must be greater than 0"
            ),
            (
                SwarmConfig {
                    connect_timeout: 0,
                    ..SwarmConfig::default()
                },
                "connect_timeout must be greater than 0"
            ),
            (
                SwarmConfig {
                    max_retries: 0,
                    ..SwarmConfig::default()
                },
                "max_retries must be greater than 0"
            ),
            (
                SwarmConfig {
                    valid_model_prefixes: vec![],
                    ..SwarmConfig::default()
                },
                "valid_model_prefixes cannot be empty"
            ),
            (
                SwarmConfig {
                    request_timeout: MIN_REQUEST_TIMEOUT - 1,
                    ..SwarmConfig::default()
                },
                "request_timeout must be between"
            ),
            (
                SwarmConfig {
                    request_timeout: MAX_REQUEST_TIMEOUT + 1,
                    ..SwarmConfig::default()
                },
                "request_timeout must be between"
            ),
        ];

        for (config, expected_error) in test_cases {
            let result = Swarm::builder()
                .with_api_key("sk-test123456789".to_string())
                .with_config(config)
                .build();

            assert!(result.is_err());
            match result {
                Err(SwarmError::ValidationError(msg)) => {
                    assert!(
                        msg.contains(expected_error),
                        "Expected error message containing '{}', got '{}'",
                        expected_error,
                        msg
                    );
                }
                _ => panic!("Expected ValidationError for invalid configuration"),
            }
        }
    }

    #[test]
    fn test_invalid_api_url() {
        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_api_url("http://invalid-url".to_string()) // Non-HTTPS URL
            .build();

        assert!(result.is_err());
        match result {
            Err(SwarmError::ValidationError(msg)) => {
                assert!(msg.contains("API URL must start with https://"));
            }
            _ => panic!("Expected ValidationError for invalid API URL"),
        }
    }

    #[test]
    fn test_valid_configurations() {
        // Test valid configuration ranges
        let valid_config = SwarmConfig {
            request_timeout: 30,
            connect_timeout: 10,
            max_retries: 3,
            valid_model_prefixes: vec!["gpt-".to_string()],
            api_url: "https://api.openai.com/v1".to_string(),
            ..SwarmConfig::default()
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_config(valid_config)
            .build();

        assert!(result.is_ok());
    }
    #[test]
    fn test_builder_basic_config() {
        let test_api_key = "sk-test123456789".to_string();

        let swarm = Swarm::builder()
            .with_api_key(test_api_key.clone())
            .build()
            .expect("Failed to build Swarm");

        assert_eq!(swarm.api_key, test_api_key);
        assert_eq!(swarm.config.api_url, OPENAI_DEFAULT_API_URL);
    }
    #[test]
    fn test_builder_api_settings() {
        let test_api_key = "sk-test123456789".to_string();
        let test_api_url = "https://api.openai.com/v2".to_string();
        let test_api_version = "2024-01".to_string();

        let swarm = Swarm::builder()
            .with_api_key(test_api_key.clone())
            .with_api_url(test_api_url.clone())
            .with_api_version(test_api_version.clone())
            .build()
            .expect("Failed to build Swarm");

        assert_eq!(swarm.api_key, test_api_key);
        assert_eq!(swarm.config.api_url, test_api_url);
        assert_eq!(swarm.config.api_version, test_api_version);
    }
    #[test]
    fn test_builder_timeout_settings() {
        let test_api_key = "sk-test123456789".to_string();
        let test_request_timeout = 60;
        let test_connect_timeout = 20;

        let swarm = Swarm::builder()
            .with_api_key(test_api_key.clone())
            .with_request_timeout(test_request_timeout)
            .with_connect_timeout(test_connect_timeout)
            .build()
            .expect("Failed to build Swarm");

        assert_eq!(swarm.config.request_timeout, test_request_timeout);
        assert_eq!(swarm.config.connect_timeout, test_connect_timeout);
    }
    #[test]
    fn test_builder_with_agent() {
        let test_api_key = "sk-test123456789".to_string();
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let swarm = Swarm::builder()
            .with_api_key(test_api_key)
            .with_agent(agent)
            .build()
            .expect("Failed to build Swarm");

        assert!(swarm.agent_registry.contains_key("test_agent"));
        assert_eq!(swarm.agent_registry["test_agent"].model, "gpt-4");
    }
    #[test]
    fn test_builder_with_custom_client() {
        let custom_client = Client::builder()
            .timeout(Duration::from_secs(45))
            .build()
            .expect("Failed to create custom client");

        let swarm = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_client(custom_client)
            .build()
            .expect("Failed to build Swarm");

        // Since we can't check the client's timeout directly,
        // we'll just verify the client was set
        assert!(Arc::strong_count(&Arc::new(swarm.client)) >= 1);
    }
    #[test]
    fn test_builder_default_values() {
        let swarm = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .build()
            .expect("Failed to build Swarm");

        let default_config = SwarmConfig::default();

        assert_eq!(swarm.config.api_url, default_config.api_url);
        assert_eq!(swarm.config.api_version, default_config.api_version);
        assert_eq!(swarm.config.request_timeout, default_config.request_timeout);
        assert_eq!(swarm.config.connect_timeout, default_config.connect_timeout);
        assert_eq!(swarm.config.max_retries, default_config.max_retries);
        assert_eq!(swarm.config.max_loop_iterations, default_config.max_loop_iterations);
        assert!(swarm.agent_registry.is_empty());
    }

    #[test]
    fn test_create_basic_agent() {
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("Basic test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        assert_eq!(agent.name, "test_agent");
        assert_eq!(agent.model, "gpt-4");
        match agent.instructions {
            Instructions::Text(text) => assert_eq!(text, "Basic test instructions"),
            _ => panic!("Expected Text instructions"),
        }
        assert!(agent.functions.is_empty());
        assert!(agent.function_call.is_none());
        assert!(!agent.parallel_tool_calls);
    }

    #[test]
    fn test_agent_with_function_instructions() {
        let instruction_fn = Arc::new(|_vars: ContextVariables| -> String {
            "Dynamic instructions".to_string()
        });

        let agent = Agent {
            name: "function_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Function(instruction_fn),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        // Test the function instructions
        let context = ContextVariables::new();
        match &agent.instructions {
            Instructions::Function(f) => assert_eq!(f(context), "Dynamic instructions"),
            _ => panic!("Expected Function instructions"),
        }
    }

    #[test]
    fn test_agent_with_functions() {
        let test_function = AgentFunction {
            name: "test_function".to_string(),
            function: Arc::new(|_: ContextVariables| -> Result<ResultType, anyhow::Error> {
                Ok(ResultType::Value("test result".to_string()))
            }),
            accepts_context_variables: false,
        };

        let agent = Agent {
            name: "function_enabled_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("Test with functions".to_string()),
            functions: vec![test_function],
            function_call: Some("auto".to_string()),
            parallel_tool_calls: true,
        };

        assert_eq!(agent.functions.len(), 1);
        assert_eq!(agent.functions[0].name, "test_function");
        assert_eq!(agent.functions[0].accepts_context_variables, false);
        assert_eq!(agent.function_call, Some("auto".to_string()));
        assert!(agent.parallel_tool_calls);
    }

    #[test]
    fn test_agent_in_swarm_registry() {
        let agent = Agent {
            name: "registry_test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let swarm = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent.clone())
            .build()
            .expect("Failed to build Swarm");

        assert!(swarm.agent_registry.contains_key(&agent.name));
        let registered_agent = swarm.agent_registry.get(&agent.name).unwrap();
        assert_eq!(registered_agent.name, "registry_test_agent");
        assert_eq!(registered_agent.model, "gpt-4");
    }

    #[test]
    fn test_agent_empty_name() {
        let agent = Agent {
            name: "".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        // Try to register the agent in a Swarm
        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        // Verify error
        assert!(result.is_err());
        match result {
            Err(SwarmError::ValidationError(msg)) => {
                assert!(msg.contains("Agent name cannot be empty"));
            }
            _ => panic!("Expected ValidationError for empty agent name"),
        }
    }

    #[test]
    fn test_agent_empty_model() {
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "".to_string(),
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        // Try to register the agent in a Swarm
        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        // Verify error
        assert!(result.is_err());
        match result {
            Err(SwarmError::ValidationError(msg)) => {
                assert!(msg.contains("Agent model cannot be empty"));
            }
            _ => panic!("Expected ValidationError for empty model"),
        }
    }

    #[test]
    fn test_agent_invalid_model_prefix() {
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "invalid-model".to_string(), // Doesn't start with valid prefix
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        // Try to register the agent in a Swarm
        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        // Verify error
        assert!(result.is_err());
        match result {
            Err(SwarmError::ValidationError(msg)) => {
                assert!(msg.contains("Invalid model prefix"));
            }
            _ => panic!("Expected ValidationError for invalid model prefix"),
        }
    }

    #[test]
    fn test_agent_missing_instructions() {
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("".to_string()), // Empty instructions
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        // Try to register the agent in a Swarm
        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        // Verify error
        assert!(result.is_err());
        match result {
            Err(SwarmError::ValidationError(msg)) => {
                assert!(msg.contains("Agent instructions cannot be empty"));
            }
            _ => panic!("Expected ValidationError for empty instructions"),
        }
    }

    #[test]
    fn test_agent_with_invalid_model_prefix() {
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "invalid-model".to_string(),
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        assert!(matches!(result, Err(SwarmError::ValidationError(_))));
        if let Err(SwarmError::ValidationError(msg)) = result {
            assert!(msg.contains("Invalid model prefix"));
        }
    }

    #[test]
    fn test_agent_with_empty_model() {
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "".to_string(),
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        assert!(matches!(result, Err(SwarmError::ValidationError(_))));
        if let Err(SwarmError::ValidationError(msg)) = result {
            assert!(msg.contains("Agent model cannot be empty"));
        }
    }

    #[test]
    fn test_agent_with_valid_model_prefix() {
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(), // Valid prefix
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        assert!(result.is_ok());
    }

    #[test]
    fn test_custom_model_prefix_validation() {
        // Create a custom config with specific model prefixes
        let config = SwarmConfig {
            valid_model_prefixes: vec!["custom-".to_string()],
            ..SwarmConfig::default()
        };

        let agent = Agent {
            name: "test_agent".to_string(),
            model: "custom-model".to_string(),
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_config(config)
            .with_agent(agent)
            .build();

        assert!(result.is_ok());
    }

    #[test]
    fn test_agent_with_valid_text_instructions() {
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("Valid test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        assert!(result.is_ok());
        if let Ok(swarm) = result {
            let stored_agent = swarm.agent_registry.get("test_agent").unwrap();
            match &stored_agent.instructions {
                Instructions::Text(text) => assert_eq!(text, "Valid test instructions"),
                _ => panic!("Expected Text instructions"),
            }
        }
    }

    #[test]
    fn test_agent_with_empty_text_instructions() {
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        assert!(result.is_err());
        match result {
            Err(SwarmError::ValidationError(msg)) => {
                assert!(msg.contains("Agent instructions cannot be empty"));
            }
            _ => panic!("Expected ValidationError for empty instructions"),
        }
    }

    #[test]
    fn test_agent_with_whitespace_only_text_instructions() {
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("    \n\t    ".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        assert!(result.is_err());
        match result {
            Err(SwarmError::ValidationError(msg)) => {
                assert!(msg.contains("Agent instructions cannot be empty"));
            }
            _ => panic!("Expected ValidationError for whitespace-only instructions"),
        }
    }

    #[test]
    fn test_agent_with_multiline_text_instructions() {
        let instructions = "Line 1\nLine 2\nLine 3".to_string();
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text(instructions.clone()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        assert!(result.is_ok());
        if let Ok(swarm) = result {
            let stored_agent = swarm.agent_registry.get("test_agent").unwrap();
            match &stored_agent.instructions {
                Instructions::Text(text) => assert_eq!(text.as_str(), instructions.as_str()),
                _ => panic!("Expected Text instructions"),
            }
        }
    }

    #[test]
    fn test_basic_function_instructions() {
        let instruction_fn = Arc::new(|_: ContextVariables| -> String {
            "Basic function instructions".to_string()
        });

        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Function(instruction_fn),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let context = ContextVariables::new();
        match &agent.instructions {
            Instructions::Function(f) => assert_eq!(f(context), "Basic function instructions"),
            _ => panic!("Expected Function instructions"),
        }
    }

    #[test]
    fn test_function_instructions_with_context() {
        let instruction_fn = Arc::new(|vars: ContextVariables| -> String {
            match vars.get("test_key") {
                Some(value) => format!("Context value: {}", value),
                None => "No context value found".to_string(),
            }
        });

        let agent = Agent {
            name: "context_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Function(instruction_fn),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let mut context = ContextVariables::new();
        context.insert("test_key".to_string(), "test_value".to_string());

        match &agent.instructions {
            Instructions::Function(f) => assert_eq!(f(context), "Context value: test_value"),
            _ => panic!("Expected Function instructions"),
        }
    }

    #[test]
    fn test_function_instructions_in_swarm() {
        let instruction_fn = Arc::new(|_: ContextVariables| -> String {
            "Swarm function instructions".to_string()
        });

        let agent = Agent {
            name: "swarm_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Function(instruction_fn),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let swarm = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build()
            .expect("Failed to build Swarm");

        let context = ContextVariables::new();
        let stored_agent = swarm.agent_registry.get("swarm_agent").unwrap();

        match &stored_agent.instructions {
            Instructions::Function(f) => assert_eq!(f(context), "Swarm function instructions"),
            _ => panic!("Expected Function instructions"),
        }
    }

    #[test]
    fn test_complex_function_instructions() {
        let instruction_fn = Arc::new(|vars: ContextVariables| -> String {
            let mut parts = Vec::new();

            if let Some(name) = vars.get("name") {
                parts.push(format!("Name: {}", name));
            }

            if let Some(role) = vars.get("role") {
                parts.push(format!("Role: {}", role));
            }

            if parts.is_empty() {
                "Default instructions".to_string()
            } else {
                parts.join("\n")
            }
        });

        let agent = Agent {
            name: "complex_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Function(instruction_fn),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        // Test with empty context
        let empty_context = ContextVariables::new();
        match &agent.instructions {
            Instructions::Function(f) => assert_eq!(f(empty_context), "Default instructions"),
            _ => panic!("Expected Function instructions"),
        }

        // Test with partial context
        let mut partial_context = ContextVariables::new();
        partial_context.insert("name".to_string(), "Test Name".to_string());
        match &agent.instructions {
            Instructions::Function(f) => assert_eq!(f(partial_context), "Name: Test Name"),
            _ => panic!("Expected Function instructions"),
        }

        // Test with full context
        let mut full_context = ContextVariables::new();
        full_context.insert("name".to_string(), "Test Name".to_string());
        full_context.insert("role".to_string(), "Test Role".to_string());
        match &agent.instructions {
            Instructions::Function(f) => assert_eq!(f(full_context), "Name: Test Name\nRole: Test Role"),
            _ => panic!("Expected Function instructions"),
        }
    }
}