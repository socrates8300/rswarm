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
        // Get API key from builder or environment
        let api_key = self.api_key.unwrap_or_else(|| {
            env::var("OPENAI_API_KEY")
                .unwrap_or_else(|_| panic!("OPENAI_API_KEY must be set either in environment or passed to builder"))
        });

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
    #[allow(dead_code)]
    fn validate(&self) -> SwarmResult<()> {
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
        Ok(())
    }
}
