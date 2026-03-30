/*
   File: rswarm/src/core.rs

   This file implements the main Swarm struct that handles chat completions,
   message history, function calls and step execution. Note that agent function
   calls are now asynchronous and are awaited without blocking the runtime.
*/

use crate::constants::{CTX_VARS_NAME, MAX_REQUEST_TIMEOUT, MIN_REQUEST_TIMEOUT};
use crate::error::{SwarmError, SwarmResult};
use crate::event::{AgentEvent, EventSubscriber};
use crate::provider::{CompletionRequest, LlmProvider, OpenAiProvider};
use crate::tool::InvocationArgs;
use crate::types::{
    Agent, AgentFunction, ApiKey, ApiUrl, ChatCompletionResponse, ContextVariables, FunctionCall,
    FunctionCallPolicy, Instructions, Message, ModelId, OpenAIErrorResponse, Response,
    ResultType, Step, Steps, SwarmConfig,
};
use crate::util::{debug_print, extract_xml_steps, function_to_json, parse_steps_from_xml};
use crate::validation::validate_api_request;
use chrono::Utc;
use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::{Duration, Instant};

impl Default for Swarm {
    fn default() -> Self {
        Swarm::new(None, None, HashMap::new()).expect("Default initialization should never fail")
    }
}

/// Main struct for managing AI agent interactions and chat completions.
pub struct Swarm {
    client: Client,
    api_key: ApiKey,
    agent_registry: HashMap<String, Agent>,
    config: SwarmConfig,
    provider: Arc<dyn LlmProvider>,
    subscribers: Vec<Arc<dyn EventSubscriber>>,
}

/// Builder pattern implementation for creating Swarm instances.
pub struct SwarmBuilder {
    client: Option<Client>,
    api_key: Option<ApiKey>,
    agents: HashMap<String, Agent>,
    config: SwarmConfig,
    build_error: Option<SwarmError>,
    subscribers: Vec<Arc<dyn EventSubscriber>>,
}

impl SwarmBuilder {
    pub fn new() -> Self {
        let config = SwarmConfig::default();
        SwarmBuilder {
            client: None,
            api_key: None,
            agents: HashMap::new(),
            config,
            build_error: None,
            subscribers: Vec::new(),
        }
    }

    pub fn with_subscriber(mut self, sub: Arc<dyn EventSubscriber>) -> Self {
        self.subscribers.push(sub);
        self
    }

    pub fn with_config(mut self, config: SwarmConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_api_url(mut self, api_url: String) -> Self {
        if let Err(err) = self.config.set_api_url(api_url) {
            self.record_error(err);
        }
        self
    }

    pub fn with_api_version(mut self, version: String) -> Self {
        if let Err(err) = self.config.set_api_version(version) {
            self.record_error(err);
        }
        self
    }

    pub fn with_request_timeout(mut self, timeout: u64) -> Self {
        if let Err(err) = self.config.set_request_timeout(timeout) {
            self.record_error(err);
        }
        self
    }

    pub fn with_connect_timeout(mut self, timeout: u64) -> Self {
        if let Err(err) = self.config.set_connect_timeout(timeout) {
            self.record_error(err);
        }
        self
    }

    pub fn with_max_retries(mut self, retries: u32) -> Self {
        if let Err(err) = self.config.set_max_retries(retries) {
            self.record_error(err);
        }
        self
    }

    pub fn with_max_loop_iterations(mut self, iterations: u32) -> Self {
        if let Err(err) = self.config.set_max_loop_iterations(iterations) {
            self.record_error(err);
        }
        self
    }

    pub fn with_valid_model_prefixes(mut self, prefixes: Vec<String>) -> Self {
        if let Err(err) = self.config.set_valid_model_prefixes(prefixes) {
            self.record_error(err);
        }
        self
    }

    pub fn with_valid_api_url_prefixes(mut self, prefixes: Vec<String>) -> Self {
        if let Err(err) = self.config.set_valid_api_url_prefixes(prefixes) {
            self.record_error(err);
        }
        self
    }

    pub fn with_client(mut self, client: Client) -> Self {
        self.client = Some(client);
        self
    }

    pub fn with_api_key(mut self, api_key: String) -> Self {
        match ApiKey::new(api_key) {
            Ok(api_key) => self.api_key = Some(api_key),
            Err(err) => self.record_error(err),
        }
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
        if let Some(err) = self.build_error {
            return Err(err);
        }

        self.config.validate()?;

        for agent in self.agents.values() {
            agent.validate(&self.config)?;
        }

        let api_key = match self.api_key {
            Some(key) => key,
            None => {
                match env::var("OPENAI_API_KEY") {
                    Ok(key) => ApiKey::new(key)?,
                    Err(_) => {
                        return Err(SwarmError::ValidationError(
                            "API key must be set either in environment or passed to builder"
                                .to_string(),
                        ))
                    }
                }
            }
        };

        let client = self.client.unwrap_or_else(|| {
            Client::builder()
                .timeout(Duration::from_secs(self.config.request_timeout()))
                .connect_timeout(Duration::from_secs(self.config.connect_timeout()))
                .build()
                .unwrap_or_else(|_| Client::new())
        });

        let provider: Arc<dyn LlmProvider> = Arc::new(OpenAiProvider::new(
            client.clone(),
            api_key.as_str(),
            self.config.api_url(),
        ));

        Ok(Swarm {
            client,
            api_key,
            agent_registry: self.agents,
            config: self.config,
            provider,
            subscribers: self.subscribers,
        })
    }

    fn record_error(&mut self, err: SwarmError) {
        if self.build_error.is_none() {
            self.build_error = Some(err);
        }
    }

    fn _validate(&self) -> SwarmResult<()> {
        if let Some(err) = &self.build_error {
            return Err(SwarmError::Other(err.to_string()));
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
        agents: HashMap<String, Agent>,
    ) -> SwarmResult<Self> {
        let mut builder = SwarmBuilder::new();

        if let Some(client) = client {
            builder = builder.with_client(client);
        }
        if let Some(api_key) = api_key {
            builder = builder.with_api_key(api_key);
        }
        builder = builder.with_agents(&agents);

        builder.build()
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn api_key(&self) -> &ApiKey {
        &self.api_key
    }

    pub fn agents(&self) -> &HashMap<String, Agent> {
        &self.agent_registry
    }

    pub fn config(&self) -> &SwarmConfig {
        &self.config
    }

    pub fn provider(&self) -> &Arc<dyn LlmProvider> {
        &self.provider
    }

    /// Emit an event to all registered subscribers.
    async fn emit(&self, event: AgentEvent) {
        for sub in &self.subscribers {
            sub.on_event(&event).await;
        }
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
        if history.is_empty() {
            return Err(SwarmError::ValidationError(
                "Message history cannot be empty".to_string(),
            ));
        }

        let instructions = match &agent.instructions {
            Instructions::Text(text) => text.clone(),
            Instructions::Function(func) => func(context_variables.clone()),
        };

        let mut messages = vec![Message::system(instructions)?];

        messages.extend_from_slice(history);

        debug_print(
            debug,
            &format!("Getting chat completion with messages: {:?}", messages),
        );

        let model = model_override.unwrap_or_else(|| agent.model.clone());

        if stream {
            // Streaming path: keep legacy HTTP implementation with functions support.
            let functions: Vec<Value> = agent
                .functions
                .iter()
                .map(function_to_json)
                .collect::<SwarmResult<Vec<Value>>>()?;

            let mut request_body = json!({
                "model": model,
                "messages": messages,
            });

            if !functions.is_empty() {
                request_body["functions"] = Value::Array(functions);
            }

            if let Some(function_call) = agent.function_call().to_wire_value() {
                request_body["function_call"] = json!(function_call);
            }

            request_body["stream"] = json!(true);

            let url = env::var("OPENAI_API_URL")
                .map(|url| {
                    ApiUrl::new(url, self.config.valid_api_url_prefixes())
                        .map(|url| url.as_str().to_string())
                })
                .unwrap_or_else(|_| Ok(self.config.api_url().to_string()))?;

            let response = self
                .client
                .post(url)
                .bearer_auth(self.api_key.as_str())
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
            // Non-streaming path: delegate to provider, then map response via JSON round-trip.
            let request = CompletionRequest::new(model, messages);
            let provider_response = self.provider.complete(request).await?;
            debug_print(debug, &format!("Provider Response: {:?}", provider_response));
            let json_val = serde_json::to_value(&provider_response).map_err(|e| {
                SwarmError::DeserializationError(format!(
                    "Failed to serialize provider response: {}",
                    e
                ))
            })?;
            serde_json::from_value(json_val)
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
        if function_call.name().trim().is_empty() {
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
            termination_reason: None,
        };

        if let Some(func) = function_map.get(function_call.name()) {
            let invocation_args = InvocationArgs::from_json_str(function_call.arguments())
                .map_err(|error| SwarmError::ValidationError(error.to_string()))?;
            let args: ContextVariables = invocation_args
                .to_context_variables()
                .map_err(|error| SwarmError::ValidationError(error.to_string()))?;
            debug_print(
                debug,
                &format!(
                    "Processing function call: {} with arguments {:?}",
                    function_call.name(),
                    args
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
            match result {
                ResultType::Value(value) => response
                    .messages
                    .push(Message::function(function_call.name(), value)?),
                ResultType::Agent(agent) => {
                    response.agent = Some(agent);
                }
                ResultType::ContextVariables(context) => {
                    response.context_variables.extend(context);
                }
                ResultType::Termination(reason) => {
                    response.termination_reason = Some(reason);
                }
            }
        } else {
            debug_print(
                debug,
                &format!("Function {} not found.", function_call.name()),
            );
            response.messages.push(Message::assistant_named(
                function_call.name(),
                format!("Error: Function {} not found.", function_call.name()),
            )?);
        }
        Ok(response)
    }

    /// Handles the result of a function call.
    pub fn handle_function_result(
        &self,
        result: ResultType,
        debug: bool,
    ) -> SwarmResult<ResultType> {
        debug_print(debug, &format!("Handling function result: {:?}", result));
        Ok(result)
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
        trace_id: &str,
    ) -> SwarmResult<Response> {
        let model = model_override.as_deref().unwrap_or(&agent.model).to_string();
        let prompt_tokens = history.len() * 50; // rough estimate

        self.emit(AgentEvent::LlmRequest {
            trace_id: trace_id.to_string(),
            model: model.clone(),
            prompt_tokens,
            timestamp: Utc::now(),
        })
        .await;

        let start = Instant::now();
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
        let latency_ms = start.elapsed().as_millis() as u64;

        if completion.choices.is_empty() {
            return Err(SwarmError::ApiError(
                "No choices returned from the model".to_string(),
            ));
        }

        let completion_tokens = completion
            .choices
            .first()
            .and_then(|c| c.message.content().as_ref().map(|s| s.len() / 4))
            .unwrap_or(0);

        self.emit(AgentEvent::LlmResponse {
            trace_id: trace_id.to_string(),
            model,
            completion_tokens,
            latency_ms,
            timestamp: Utc::now(),
        })
        .await;

        let choice = &completion.choices[0];
        let message = choice.message.clone();
        history.push(message.clone());

        if let Some(function_call) = message.function_call() {
            self.emit(AgentEvent::ToolCall {
                trace_id: trace_id.to_string(),
                tool_name: function_call.name().to_string(),
                arguments: serde_json::from_str(function_call.arguments())
                    .unwrap_or(Value::Null),
                timestamp: Utc::now(),
            })
            .await;

            let tool_start = Instant::now();
            let func_response = self
                .handle_function_call(function_call, &agent.functions, context_variables, debug)
                .await?;
            let tool_duration_ms = tool_start.elapsed().as_millis() as u64;

            self.emit(AgentEvent::ToolResult {
                trace_id: trace_id.to_string(),
                tool_name: function_call.name().to_string(),
                result: func_response
                    .messages
                    .first()
                    .and_then(|m| m.content().map(|c| Value::String(c.to_string())))
                    .unwrap_or(Value::Null),
                success: true,
                duration_ms: tool_duration_ms,
                timestamp: Utc::now(),
            })
            .await;

            history.extend(func_response.messages.clone());
            context_variables.extend(func_response.context_variables);
        }

        Ok(Response {
            messages: vec![message],
            agent: Some(agent.clone()),
            context_variables: context_variables.clone(),
            termination_reason: None,
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
        trace_id: &str,
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

        match step.action {
            crate::types::StepAction::RunOnce => {
                let step_message = Message::user(step.prompt.clone())?;
                history.push(step_message);
                self.single_execution(
                    agent,
                    history,
                    context_variables,
                    model_override.clone(),
                    stream,
                    debug,
                    trace_id,
                )
                .await
            }
            crate::types::StepAction::Loop => {
                let mut iteration_count: usize = 0;
                loop {
                    if iteration_count >= max_turns {
                        return Err(SwarmError::MaxIterationsError {
                            max: max_turns,
                            actual: iteration_count,
                        });
                    }
                    iteration_count += 1;
                    let step_message = Message::user(step.prompt.clone())?;
                    history.push(step_message);
                    let response = self
                        .single_execution(
                            agent,
                            history,
                            context_variables,
                            model_override.clone(),
                            stream,
                            debug,
                            trace_id,
                        )
                        .await?;
                    if let Some(new_agent) = response.agent.clone() {
                        *agent = new_agent;
                    }
                    if let Some(reason) = response.termination_reason {
                        debug_print(debug, &format!("Loop terminated: {}", reason));
                        break;
                    }
                }
                Ok(Response {
                    messages: history.clone(),
                    agent: Some(agent.clone()),
                    context_variables: context_variables.clone(),
                    termination_reason: None,
                })
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

        if max_turns > self.config.max_loop_iterations() as usize {
            return Err(SwarmError::ValidationError(format!(
                "max_turns ({}) exceeds configured max_loop_iterations ({})",
                max_turns,
                self.config.max_loop_iterations()
            )));
        }

        let trace_id = uuid::Uuid::new_v4().to_string();

        self.emit(AgentEvent::LoopStart {
            trace_id: trace_id.clone(),
            agent_name: agent.name().to_string(),
            timestamp: Utc::now(),
        })
        .await;

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
        let mut iterations: u32 = 0;

        if !steps.steps.is_empty() {
            for step in &steps.steps {
                iterations += 1;
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
                        &trace_id,
                    )
                    .await?;
                if let Some(new_agent) = response.agent {
                    agent = new_agent;
                }
            }
        } else {
            debug_print(debug, "No steps defined. Executing default behavior.");
            iterations += 1;
            let response = self
                .single_execution(
                    &agent,
                    &mut history,
                    &mut context_variables,
                    model_override,
                    stream,
                    debug,
                    &trace_id,
                )
                .await?;
            history.extend(response.messages);
            context_variables.extend(response.context_variables);
        }

        self.emit(AgentEvent::LoopEnd {
            trace_id,
            agent_name: agent.name().to_string(),
            iterations,
            total_tokens: 0,
            termination_reason: crate::phase::TerminationReason::TaskComplete,
            timestamp: Utc::now(),
        })
        .await;

        Ok(Response {
            messages: history,
            agent: Some(agent),
            context_variables,
            termination_reason: None,
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
        if self.valid_model_prefixes().is_empty() {
            return Err(SwarmError::ValidationError(
                "valid_model_prefixes cannot be empty".to_string(),
            ));
        }
        if self.valid_api_url_prefixes().is_empty() {
            return Err(SwarmError::ValidationError(
                "valid_api_url_prefixes cannot be empty".to_string(),
            ));
        }
        if self.request_timeout() < MIN_REQUEST_TIMEOUT || self.request_timeout() > MAX_REQUEST_TIMEOUT
        {
            return Err(SwarmError::ValidationError(format!(
                "request_timeout must be between {} and {} seconds",
                MIN_REQUEST_TIMEOUT, MAX_REQUEST_TIMEOUT
            )));
        }
        if self.loop_control().default_max_iterations == 0 {
            return Err(SwarmError::ValidationError(
                "default_max_iterations must be greater than 0".to_string(),
            ));
        }
        Ok(())
    }
}

impl Agent {
    pub fn validate(&self, config: &SwarmConfig) -> SwarmResult<()> {
        self.validate_intrinsic_fields()?;
        ModelId::new(self.model.clone(), config.valid_model_prefixes())?;
        match self.function_call() {
            FunctionCallPolicy::Disabled => {}
            FunctionCallPolicy::Auto => {
                if self.functions().is_empty() {
                    return Err(SwarmError::ValidationError(
                        "Function call policy requires at least one registered function"
                            .to_string(),
                    ));
                }
            }
            FunctionCallPolicy::Named(name) => {
                if name.trim().is_empty() {
                    return Err(SwarmError::ValidationError(
                        "Named function call policy cannot be empty".to_string(),
                    ));
                }
                if !self.functions().iter().any(|function| function.name == *name) {
                    return Err(SwarmError::ValidationError(format!(
                        "Named function call policy references unknown function: {}",
                        name
                    )));
                }
            }
        }
        match self.instructions() {
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
