/*
   File: rswarm/src/core.rs

   This file implements the main Swarm struct that handles chat completions,
   message history, function calls and step execution. Note that agent function
   calls are now asynchronous and are awaited without blocking the runtime.
*/

use crate::constants::{
    CTX_VARS_NAME, MAX_REQUEST_TIMEOUT, MIN_REQUEST_TIMEOUT, OPENAI_DEFAULT_API_URL,
    ROLE_ASSISTANT, ROLE_FUNCTION, ROLE_SYSTEM,
};
use crate::error::{SwarmError, SwarmResult};
use crate::types::{
    Agent, AgentFunction, ChatCompletionResponse, ContextVariables, FunctionCall, Instructions,
    Message, OpenAIErrorResponse, Response, ResultType, Step, Steps, SwarmConfig,
};
use crate::util::{debug_print, extract_xml_steps, function_to_json, parse_steps_from_xml};
use crate::validation::{validate_api_request, validate_api_url};
use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::time::Duration;

impl Default for Swarm {
    fn default() -> Self {
        Swarm::new(None, None, HashMap::new()).expect("Default initialization should never fail")
    }
}

/// Main struct for managing AI agent interactions and chat completions.
pub struct Swarm {
    pub client: Client,
    pub api_key: String,
    pub agent_registry: HashMap<String, Agent>,
    pub config: SwarmConfig,
}

/// Builder pattern implementation for creating Swarm instances.
pub struct SwarmBuilder {
    client: Option<Client>,
    api_key: Option<String>,
    agents: HashMap<String, Agent>,
    config: SwarmConfig,
}

impl SwarmBuilder {
    pub fn new() -> Self {
        let config = SwarmConfig::default();
        SwarmBuilder {
            client: None,
            api_key: None,
            agents: HashMap::new(),
            config,
        }
    }

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

    pub fn with_agents(mut self, agents: &HashMap<String, Agent>) -> Self {
        for agent in agents.values() {
            self = self.with_agent(agent.clone());
        }
        self
    }

    pub fn build(self) -> SwarmResult<Swarm> {
        self.config.validate()?;

        for agent in self.agents.values() {
            agent.validate(&self.config)?;
        }

        let api_key = match self.api_key.or_else(|| env::var("OPENAI_API_KEY").ok()) {
            Some(key) => key,
            None => {
                return Err(SwarmError::ValidationError(
                    "API key must be set either in environment or passed to builder".to_string(),
                ))
            }
        };

        if api_key.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "API key cannot be empty".to_string(),
            ));
        }
        if !api_key.starts_with("sk-") {
            return Err(SwarmError::ValidationError(
                "Invalid API key format".to_string(),
            ));
        }

        let api_url = self.config.api_url.clone();

        if !api_url.starts_with("https://")
            && !api_url.starts_with("http://localhost")
            && !api_url.starts_with("http://127.0.0.1")
        {
            return Err(SwarmError::ValidationError(
                "API URL must start with https:// (except for localhost)".to_string(),
            ));
        }

        validate_api_url(&api_url, &self.config)?;

        let client = self.client.unwrap_or_else(|| {
            Client::builder()
                .timeout(Duration::from_secs(self.config.request_timeout))
                .connect_timeout(Duration::from_secs(self.config.connect_timeout))
                .build()
                .unwrap_or_else(|_| Client::new())
        });

        Ok(Swarm {
            client,
            api_key,
            agent_registry: self.agents,
            config: self.config,
        })
    }

    fn _validate(&self) -> SwarmResult<()> {
        if let Some(ref key) = self.api_key {
            if key.trim().is_empty() || !key.starts_with("sk-") {
                return Err(SwarmError::ValidationError(
                    "Invalid API key format".to_string(),
                ));
            }
        }
        self.config.validate()?;
        Ok(())
    }
}

impl Swarm {
    pub fn builder() -> SwarmBuilder {
        SwarmBuilder::new()
    }

    // For backward compatibility.
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

    /// Makes an asynchronous chat completion request.
    pub async fn get_chat_completion(
        &self,
        agent: &Agent,
        history: &[Message],
        context_variables: &ContextVariables,
        model_override: Option<String>,
        stream: bool,
        debug: bool,
    ) -> SwarmResult<ChatCompletionResponse> {
        if self.api_key.is_empty() {
            return Err(SwarmError::ValidationError(
                "API key cannot be empty".to_string(),
            ));
        }

        if history.is_empty() {
            return Err(SwarmError::ValidationError(
                "Message history cannot be empty".to_string(),
            ));
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
            &format!("Getting chat completion with messages: {:?}", messages),
        );

        let functions: Vec<Value> = agent
            .functions
            .iter()
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
                if url.starts_with("http://localhost") || url.starts_with("http://127.0.0.1") {
                    Ok(url)
                } else if !url.starts_with("https://") {
                    Err(SwarmError::ValidationError(
                        "OPENAI_API_URL must start with https:// (except for localhost)"
                            .to_string(),
                    ))
                } else {
                    Ok(url)
                }
            })
            .unwrap_or_else(|_| Ok(OPENAI_DEFAULT_API_URL.to_string()))?;

        let response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| SwarmError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let error_text = response.text().await.map_err(|e| {
                SwarmError::NetworkError(format!("Failed to read error response: {}", e))
            })?;
            debug_print(debug, &format!("API Error Response: {}", error_text));
            let api_error: serde_json::Result<OpenAIErrorResponse> =
                serde_json::from_str(&error_text);
            return match api_error {
                Ok(err_resp) => Err(SwarmError::ApiError(err_resp.error.message)),
                Err(_) => Err(SwarmError::ApiError(error_text)),
            };
        }

        if stream {
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
                                    full_response.choices.extend(partial.choices);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        return Err(SwarmError::StreamError(format!(
                            "Error reading streaming response: {}",
                            e
                        )))
                    }
                }
            }
            Ok(full_response)
        } else {
            let response_text = response.text().await.map_err(|e| {
                SwarmError::DeserializationError(format!("Failed to read response text: {}", e))
            })?;
            debug_print(debug, &format!("API Response: {}", response_text));
            serde_json::from_str::<ChatCompletionResponse>(&response_text)
                .map_err(|e| SwarmError::DeserializationError(e.to_string()))
        }
    }

    /// Asynchronously handles a function call from an agent.
    pub async fn handle_function_call(
        &self,
        function_call: &FunctionCall,
        functions: &[AgentFunction],
        context_variables: &mut ContextVariables,
        debug: bool,
    ) -> SwarmResult<Response> {
        if function_call.name.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "Function call name cannot be empty.".to_string(),
            ));
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

        if let Some(func) = function_map.get(&function_call.name) {
            let args: ContextVariables = serde_json::from_str(&function_call.arguments)?;
            debug_print(
                debug,
                &format!(
                    "Processing function call: {} with arguments {:?}",
                    function_call.name, args
                ),
            );

            let mut args = args.clone();
            if func.accepts_context_variables {
                let serialized_context = serde_json::to_string(&context_variables)?;
                args.insert(CTX_VARS_NAME.to_string(), serialized_context);
            }

            // Await the asynchronous call.
            let raw_result = (func.function)(args).await?;
            let result = self.handle_function_result(raw_result, debug)?;
            response.messages.push(Message {
                role: ROLE_FUNCTION.to_string(),
                name: Some(function_call.name.clone()),
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
            debug_print(
                debug,
                &format!("Function {} not found.", function_call.name),
            );
            response.messages.push(Message {
                role: ROLE_ASSISTANT.to_string(),
                name: Some(function_call.name.clone()),
                content: Some(format!("Error: Function {} not found.", function_call.name)),
                function_call: None,
            });
        }
        Ok(response)
    }

    /// Handles the result of a function call.
    pub fn handle_function_result(
        &self,
        result: ResultType,
        debug: bool,
    ) -> SwarmResult<ResultType> {
        match result {
            ResultType::Value(_) | ResultType::Agent(_) => Ok(result),
            _ => {
                let error_message = format!(
                    "Failed to cast response to string: {:?}. Ensure agent functions return a string or ResultType.",
                    result
                );
                debug_print(debug, &error_message);
                Err(SwarmError::FunctionError(error_message))
            }
        }
    }

    /// Executes a single round of conversation with the agent.
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
            return Err(SwarmError::ApiError(
                "No choices returned from the model".to_string(),
            ));
        }
        let choice = &completion.choices[0];
        let message = choice.message.clone();
        history.push(message.clone());

        if let Some(function_call) = &message.function_call {
            // Await the asynchronous function call handler.
            let func_response = self
                .handle_function_call(function_call, &agent.functions, context_variables, debug)
                .await?;
            history.extend(func_response.messages.clone());
            context_variables.extend(func_response.context_variables);
        }

        Ok(Response {
            messages: vec![message],
            agent: Some(agent.clone()),
            context_variables: context_variables.clone(),
        })
    }

    /// Executes a step based on the provided XML–defined step.
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
        if step.prompt.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "Step prompt cannot be empty".to_string(),
            ));
        }
        if step.number == 0 {
            return Err(SwarmError::ValidationError(
                "Step number must be greater than 0".to_string(),
            ));
        }

        println!("Executing Step {}", step_number);

        if let Some(agent_name) = &step.agent {
            println!("Switching to agent: {}", agent_name);
            *agent = self.get_agent_by_name(agent_name)?;
        }

        match step.action.as_str() {
            "run_once" => {
                let step_message = Message {
                    role: "user".to_string(),
                    content: Some(step.prompt.clone()),
                    name: None,
                    function_call: None,
                };
                history.push(step_message);
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
                let max_iterations = 10;
                loop {
                    iteration_count += 1;
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
                    if let Some(new_agent) = response.agent.clone() {
                        *agent = new_agent;
                    }
                    if context_variables.get("end_loop") == Some(&"true".to_string()) {
                        println!("Loop termination condition met.");
                        break;
                    }
                    if iteration_count >= max_iterations {
                        println!("Reached maximum loop iterations.");
                        break;
                    }
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
                Err(SwarmError::ValidationError(format!(
                    "Unknown action: {}",
                    step.action
                )))
            }
        }
    }

    /// Executes a multi-turn conversation with the AI agent.
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
        validate_api_request(&agent, &messages, &model_override, max_turns)?;

        if max_turns > self.config.max_loop_iterations as usize {
            return Err(SwarmError::ValidationError(format!(
                "max_turns ({}) exceeds configured max_loop_iterations ({})",
                max_turns, self.config.max_loop_iterations
            )));
        }

        let instructions = match &agent.instructions {
            Instructions::Text(text) => text.clone(),
            Instructions::Function(func) => func(context_variables.clone()),
        };

        let (instructions_without_xml, xml_steps) = extract_xml_steps(&instructions)?;

        let steps = if let Some(xml_content) = xml_steps {
            parse_steps_from_xml(&xml_content)?
        } else {
            Steps { steps: Vec::new() }
        };

        agent.instructions = Instructions::Text(instructions_without_xml);
        let mut history = messages.clone();

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
                if let Some(new_agent) = response.agent {
                    agent = new_agent;
                }
            }
        } else {
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

    pub fn get_agent_by_name(&self, name: &str) -> SwarmResult<Agent> {
        self.agent_registry
            .get(name)
            .cloned()
            .ok_or_else(|| SwarmError::AgentNotFoundError(name.to_string()))
    }
}

impl SwarmConfig {
    pub fn validate(&self) -> SwarmResult<()> {
        if self.request_timeout == 0 {
            return Err(SwarmError::ValidationError(
                "request_timeout must be greater than 0".to_string(),
            ));
        }
        if self.connect_timeout == 0 {
            return Err(SwarmError::ValidationError(
                "connect_timeout must be greater than 0".to_string(),
            ));
        }
        if self.max_retries == 0 {
            return Err(SwarmError::ValidationError(
                "max_retries must be greater than 0".to_string(),
            ));
        }
        if self.valid_model_prefixes.is_empty() {
            return Err(SwarmError::ValidationError(
                "valid_model_prefixes cannot be empty".to_string(),
            ));
        }
        if self.request_timeout < MIN_REQUEST_TIMEOUT || self.request_timeout > MAX_REQUEST_TIMEOUT
        {
            return Err(SwarmError::ValidationError(format!(
                "request_timeout must be between {} and {} seconds",
                MIN_REQUEST_TIMEOUT, MAX_REQUEST_TIMEOUT
            )));
        }
        if self.loop_control.default_max_iterations == 0 {
            return Err(SwarmError::ValidationError(
                "default_max_iterations must be greater than 0".to_string(),
            ));
        }
        Ok(())
    }
}

impl Agent {
    pub fn validate(&self, config: &SwarmConfig) -> SwarmResult<()> {
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
        if !config
            .valid_model_prefixes
            .iter()
            .any(|prefix| self.model.starts_with(prefix))
        {
            return Err(SwarmError::ValidationError(format!(
                "Invalid model prefix. Model must start with one of: {:?}",
                config.valid_model_prefixes
            )));
        }
        match &self.instructions {
            Instructions::Text(text) if text.trim().is_empty() => {
                return Err(SwarmError::ValidationError(
                    "Agent instructions cannot be empty".to_string(),
                ));
            }
            Instructions::Function(_) => {} // Function instructions are validated at runtime.
            _ => {}
        }
        Ok(())
    }
}
