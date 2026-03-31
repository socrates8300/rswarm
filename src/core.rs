/*
   File: rswarm/src/core.rs

   This file implements the main Swarm struct that handles chat completions,
   message history, function calls and step execution. Note that agent function
   calls are now asynchronous and are awaited without blocking the runtime.
*/

use crate::agent_comm::{AgentMessage, ChannelRegistry, InProcessChannel};
use crate::agent_registry::AgentRegistry;
use crate::checkpoint::{CheckpointData, CheckpointEnvelope};
use crate::circuit_breaker::{CircuitBreaker, CircuitStateSnapshot};
use crate::constants::{CTX_VARS_NAME, MAX_REQUEST_TIMEOUT, MIN_REQUEST_TIMEOUT};
use crate::distribution::{
    AgentAddress, DistributedMessage, DistributedTransport, HttpDistributedTransport,
};
use crate::error::{SwarmError, SwarmResult};
use crate::escalation::{
    EscalationAction, EscalationConfig, EscalationDetector, EscalationTrigger,
};
use crate::event::{AgentEvent, EventSubscriber, TraceId};
use crate::guardrails::{
    check_injection_with_policy, classify_and_redact, ContentPolicy, DataClassification,
    DefaultContentPolicy, InjectionOutcome, InjectionPolicy, PolicyResult, RedactionPolicy,
};
use crate::observability::{
    record_budget_exhausted, record_circuit_breaker_state, record_guardrail_triggered,
    record_iteration, record_llm_latency, record_token_usage, record_tool_call,
};
use crate::persistence::{
    CheckpointStore, EventStore, MemoryStore, PersistenceBackend, SessionStore,
};
use crate::phase::TokenUsage;
use crate::provider::{CompletionRequest, LlmProvider, OpenAiProvider};
use crate::team::{
    AgentTeam, ConsensusStrategy, TeamAssignment, TeamDecision, TeamFormationPolicy, TeamRole,
    TeamVote, VoteTally,
};
use crate::tool::InvocationArgs;
use crate::types::{
    Agent, AgentFunction, AgentRef, ApiKey, ApiUrl, ChatCompletionResponse, Choice,
    ContextVariables, FinishReason, FunctionCall, FunctionCallPolicy, Instructions, Message,
    MessageRole, ModelId, OpenAIErrorResponse, Response, ResultType, RuntimeLimits, Step, Steps,
    SwarmConfig, ToolCall, ToolCallExecution,
};
use crate::util::{debug_print, extract_xml_steps, function_to_json, parse_steps_from_xml};
use crate::validation::{
    validate_api_request, verify_structured_response, BudgetEnforcer, BudgetExhausted,
};
use chrono::Utc;
use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::env;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
struct CircuitBreakerSettings {
    failure_threshold: u32,
    reset_secs: u64,
}

impl CircuitBreakerSettings {
    fn validate(&self, label: &str) -> SwarmResult<()> {
        if self.failure_threshold == 0 {
            return Err(SwarmError::ValidationError(format!(
                "{} failure_threshold must be greater than 0",
                label
            )));
        }
        Ok(())
    }
}

impl Default for CircuitBreakerSettings {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            reset_secs: 30,
        }
    }
}

#[derive(Clone)]
struct RunOptions {
    model_override: Option<String>,
    stream: bool,
    debug: bool,
    max_turns: usize,
}

struct RunState {
    agent: Agent,
    history: Vec<Message>,
    context_variables: ContextVariables,
    iterations: u32,
    total_tokens: u32,
}

struct ExecutionContext<'a> {
    trace_id: &'a TraceId,
    options: &'a RunOptions,
    budget: &'a mut BudgetEnforcer,
    escalation: &'a mut EscalationDetector,
}

struct ToolCallOutcome {
    tool_call: ToolCall,
    response: SwarmResult<Response>,
}

fn max_classification(
    current: Option<DataClassification>,
    candidate: Option<DataClassification>,
) -> Option<DataClassification> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => Some(std::cmp::max(current, candidate)),
        (Some(current), None) => Some(current),
        (None, Some(candidate)) => Some(candidate),
        (None, None) => None,
    }
}

/// Main struct for managing AI agent interactions and chat completions.
pub struct Swarm {
    client: Client,
    api_key: ApiKey,
    agent_registry: HashMap<String, Agent>,
    agent_directory: AgentRegistry,
    channel_registry: Arc<ChannelRegistry>,
    config: SwarmConfig,
    provider: Arc<dyn LlmProvider>,
    distributed_transport: Arc<dyn DistributedTransport>,
    subscribers: Vec<Arc<dyn EventSubscriber>>,
    session_store: Option<Arc<dyn SessionStore>>,
    event_store: Option<Arc<dyn EventStore>>,
    /// Optional durable checkpoint store (task #32/#33).
    checkpoint_store: Option<Arc<dyn CheckpointStore>>,
    /// Optional durable memory store (task #38).
    memory_store: Option<Arc<dyn MemoryStore>>,
    content_policy: Arc<dyn ContentPolicy>,
    injection_policy: InjectionPolicy,
    redaction_policy: RedactionPolicy,
    redaction_threshold: DataClassification,
    escalation_config: EscalationConfig,
    provider_breaker: CircuitBreaker,
    tool_breaker_settings: CircuitBreakerSettings,
    tool_breakers: Arc<Mutex<HashMap<String, CircuitBreaker>>>,
    team_assignment_load: Arc<Mutex<HashMap<AgentRef, u64>>>,
}

/// Builder pattern implementation for creating Swarm instances.
pub struct SwarmBuilder {
    client: Option<Client>,
    api_key: Option<ApiKey>,
    agents: HashMap<String, Agent>,
    distributed_transport: Option<Arc<dyn DistributedTransport>>,
    config: SwarmConfig,
    build_error: Option<SwarmError>,
    subscribers: Vec<Arc<dyn EventSubscriber>>,
    session_store: Option<Arc<dyn SessionStore>>,
    event_store: Option<Arc<dyn EventStore>>,
    checkpoint_store: Option<Arc<dyn CheckpointStore>>,
    memory_store: Option<Arc<dyn MemoryStore>>,
    content_policy: Arc<dyn ContentPolicy>,
    injection_policy: InjectionPolicy,
    redaction_policy: RedactionPolicy,
    redaction_threshold: DataClassification,
    escalation_config: EscalationConfig,
    provider_breaker_settings: CircuitBreakerSettings,
    tool_breaker_settings: CircuitBreakerSettings,
}

impl SwarmBuilder {
    pub fn new() -> Self {
        let config = SwarmConfig::default();
        SwarmBuilder {
            client: None,
            api_key: None,
            agents: HashMap::new(),
            distributed_transport: None,
            config,
            build_error: None,
            subscribers: Vec::new(),
            session_store: None,
            event_store: None,
            checkpoint_store: None,
            memory_store: None,
            content_policy: Arc::new(DefaultContentPolicy),
            injection_policy: InjectionPolicy::default(),
            redaction_policy: RedactionPolicy::Redact,
            redaction_threshold: DataClassification::Sensitive,
            escalation_config: EscalationConfig::default(),
            provider_breaker_settings: CircuitBreakerSettings::default(),
            tool_breaker_settings: CircuitBreakerSettings::default(),
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

    pub fn with_runtime_limits(mut self, limits: RuntimeLimits) -> Self {
        self.config.set_runtime_limits(limits);
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

    pub fn with_checkpoint_store(mut self, store: Arc<dyn CheckpointStore>) -> Self {
        self.checkpoint_store = Some(store);
        self
    }

    pub fn with_session_store(mut self, store: Arc<dyn SessionStore>) -> Self {
        self.session_store = Some(store);
        self
    }

    pub fn with_event_store(mut self, store: Arc<dyn EventStore>) -> Self {
        self.event_store = Some(store);
        self
    }

    pub fn with_memory_store(mut self, store: Arc<dyn MemoryStore>) -> Self {
        self.memory_store = Some(store);
        self
    }

    pub fn with_persistence_backend<T>(mut self, backend: T) -> Self
    where
        T: PersistenceBackend + 'static,
    {
        let backend = Arc::new(backend);
        self.session_store = Some(backend.clone());
        self.event_store = Some(backend.clone());
        self.checkpoint_store = Some(backend.clone());
        self.memory_store = Some(backend);
        self
    }

    pub fn with_client(mut self, client: Client) -> Self {
        self.client = Some(client);
        self
    }

    pub fn with_distributed_transport(mut self, transport: Arc<dyn DistributedTransport>) -> Self {
        self.distributed_transport = Some(transport);
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

    pub fn with_content_policy(mut self, policy: Arc<dyn ContentPolicy>) -> Self {
        self.content_policy = policy;
        self
    }

    pub fn with_injection_policy(mut self, policy: InjectionPolicy) -> Self {
        self.injection_policy = policy;
        self
    }

    pub fn with_redaction_policy(mut self, policy: RedactionPolicy) -> Self {
        self.redaction_policy = policy;
        self
    }

    pub fn with_redaction_threshold(mut self, threshold: DataClassification) -> Self {
        self.redaction_threshold = threshold;
        self
    }

    pub fn with_escalation_config(mut self, config: EscalationConfig) -> Self {
        self.escalation_config = config;
        self
    }

    pub fn with_provider_circuit_breaker(
        mut self,
        failure_threshold: u32,
        reset_secs: u64,
    ) -> Self {
        self.provider_breaker_settings = CircuitBreakerSettings {
            failure_threshold,
            reset_secs,
        };
        self
    }

    pub fn with_tool_circuit_breaker(mut self, failure_threshold: u32, reset_secs: u64) -> Self {
        self.tool_breaker_settings = CircuitBreakerSettings {
            failure_threshold,
            reset_secs,
        };
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

        self.provider_breaker_settings
            .validate("provider circuit breaker")?;
        self.tool_breaker_settings
            .validate("tool circuit breaker")?;

        let api_key = match self.api_key {
            Some(key) => key,
            None => match env::var("OPENAI_API_KEY") {
                Ok(key) => ApiKey::new(key)?,
                Err(_) => {
                    return Err(SwarmError::ValidationError(
                        "API key must be set either in environment or passed to builder"
                            .to_string(),
                    ))
                }
            },
        };

        let client = self.client.unwrap_or_else(|| {
            Client::builder()
                .timeout(Duration::from_secs(self.config.request_timeout()))
                .connect_timeout(Duration::from_secs(self.config.connect_timeout()))
                .build()
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        "Failed to build configured HTTP client ({}), falling back to default — \
                         request/connect timeouts will not be applied",
                        e
                    );
                    Client::new()
                })
        });

        let provider: Arc<dyn LlmProvider> = Arc::new(OpenAiProvider::new(
            client.clone(),
            api_key.as_str(),
            self.config.api_url(),
        ));
        let distributed_transport = self
            .distributed_transport
            .unwrap_or_else(|| Arc::new(HttpDistributedTransport::new(client.clone())));
        let agent_directory = AgentRegistry::new();
        for agent in self.agents.values() {
            agent_directory.register(Arc::new(agent.clone()));
        }
        let channel_registry = ChannelRegistry::new();

        Ok(Swarm {
            client,
            api_key,
            agent_registry: self.agents,
            agent_directory,
            channel_registry,
            config: self.config,
            provider,
            distributed_transport,
            subscribers: self.subscribers,
            session_store: self.session_store,
            event_store: self.event_store,
            checkpoint_store: self.checkpoint_store,
            memory_store: self.memory_store,
            content_policy: self.content_policy,
            injection_policy: self.injection_policy,
            redaction_policy: self.redaction_policy,
            redaction_threshold: self.redaction_threshold,
            escalation_config: self.escalation_config,
            provider_breaker: CircuitBreaker::new(
                "provider",
                self.provider_breaker_settings.failure_threshold,
                self.provider_breaker_settings.reset_secs,
            ),
            tool_breaker_settings: self.tool_breaker_settings,
            tool_breakers: Arc::new(Mutex::new(HashMap::new())),
            team_assignment_load: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn record_error(&mut self, err: SwarmError) {
        if self.build_error.is_none() {
            self.build_error = Some(err);
        }
    }
}

impl Default for SwarmBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Swarm {
    pub fn builder() -> SwarmBuilder {
        SwarmBuilder::new()
    }

    fn next_trace_id() -> TraceId {
        TraceId::from(uuid::Uuid::new_v4().to_string())
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

    pub fn agent_directory(&self) -> &AgentRegistry {
        &self.agent_directory
    }

    pub fn channel_registry(&self) -> &Arc<ChannelRegistry> {
        &self.channel_registry
    }

    pub fn config(&self) -> &SwarmConfig {
        &self.config
    }

    pub fn provider(&self) -> &Arc<dyn LlmProvider> {
        &self.provider
    }

    pub fn find_agents_by_capability(&self, capability: &str) -> Vec<AgentRef> {
        self.agent_directory.find_by_capability(capability)
    }

    pub async fn open_agent_channel(
        &self,
        agent: impl Into<AgentRef>,
    ) -> SwarmResult<InProcessChannel> {
        let agent = agent.into();
        if self.agent_directory.get(&agent).is_none() {
            return Err(SwarmError::AgentNotFoundError(agent.to_string()));
        }
        InProcessChannel::new(agent, self.channel_registry.clone()).await
    }

    async fn emit_message_sent(
        &self,
        trace_id: &TraceId,
        from: AgentAddress,
        to: AgentAddress,
        message_id: crate::agent_comm::MessageId,
    ) {
        self.emit(AgentEvent::MessageSent {
            trace_id: trace_id.clone(),
            from,
            to,
            message_id,
            timestamp: Utc::now(),
        })
        .await;
    }

    async fn emit_message_received(
        &self,
        trace_id: &TraceId,
        by: AgentAddress,
        message_id: crate::agent_comm::MessageId,
    ) {
        self.emit(AgentEvent::MessageReceived {
            trace_id: trace_id.clone(),
            by,
            message_id,
            timestamp: Utc::now(),
        })
        .await;
    }

    async fn emit_reply_timeout(
        &self,
        trace_id: &TraceId,
        from: AgentAddress,
        to: AgentAddress,
        correlation_id: crate::agent_comm::MessageId,
        timeout: Duration,
    ) {
        self.emit(AgentEvent::ReplyTimeout {
            trace_id: trace_id.clone(),
            from,
            to,
            correlation_id,
            timeout_ms: timeout.as_millis() as u64,
            timestamp: Utc::now(),
        })
        .await;
    }

    fn validate_local_agent(&self, agent: &AgentRef) -> SwarmResult<()> {
        if self.agent_directory.get(agent).is_some() {
            Ok(())
        } else {
            Err(SwarmError::AgentNotFoundError(agent.to_string()))
        }
    }

    fn local_message_from_distributed(message: &DistributedMessage) -> SwarmResult<AgentMessage> {
        let (AgentAddress::Local { agent: from }, AgentAddress::Local { agent: to }) =
            (&message.from, &message.to)
        else {
            return Err(SwarmError::ValidationError(
                "Local message routing requires local source and destination".to_string(),
            ));
        };

        Ok(AgentMessage {
            id: message.id.clone(),
            from: from.clone(),
            to: to.clone(),
            payload: message.payload.clone(),
            timestamp: message.timestamp,
            correlation_id: message.correlation_id.clone(),
            is_reply: message.is_reply,
        })
    }

    pub async fn send_agent_message(
        &self,
        trace_id: Option<TraceId>,
        from: AgentAddress,
        to: AgentAddress,
        payload: Value,
    ) -> SwarmResult<crate::agent_comm::MessageId> {
        let trace_id = trace_id.unwrap_or_else(Self::next_trace_id);
        let message = DistributedMessage::new(from.clone(), to.clone(), payload)
            .with_trace_id(trace_id.clone());

        match (&from, &to) {
            (
                AgentAddress::Local { agent: local_from },
                AgentAddress::Local { agent: local_to },
            ) => {
                self.validate_local_agent(local_from)?;
                self.validate_local_agent(local_to)?;
                self.channel_registry
                    .send(Self::local_message_from_distributed(&message)?)
                    .await?;
            }
            _ => {
                self.distributed_transport.send(message.clone()).await?;
            }
        }

        self.emit_message_sent(&trace_id, from, to, message.id.clone())
            .await;
        Ok(message.id)
    }

    pub async fn request_agent_message(
        &self,
        trace_id: Option<TraceId>,
        from: AgentAddress,
        to: AgentAddress,
        payload: Value,
        timeout: Duration,
    ) -> SwarmResult<DistributedMessage> {
        let trace_id = trace_id.unwrap_or_else(Self::next_trace_id);
        let message = DistributedMessage::new(from.clone(), to.clone(), payload)
            .with_trace_id(trace_id.clone());

        let result = match (&from, &to) {
            (
                AgentAddress::Local { agent: local_from },
                AgentAddress::Local { agent: local_to },
            ) => {
                self.validate_local_agent(local_from)?;
                self.validate_local_agent(local_to)?;
                self.channel_registry
                    .request(Self::local_message_from_distributed(&message)?, timeout)
                    .await
                    .map(|reply| DistributedMessage {
                        id: reply.id,
                        from: AgentAddress::local(reply.from),
                        to: AgentAddress::local(reply.to),
                        payload: reply.payload,
                        timestamp: reply.timestamp,
                        correlation_id: reply.correlation_id,
                        trace_id: Some(trace_id.clone()),
                        is_reply: reply.is_reply,
                    })
            }
            _ => {
                self.distributed_transport
                    .request(message.clone(), timeout)
                    .await
            }
        };

        match result {
            Ok(reply) => {
                self.emit_message_sent(&trace_id, from, to, message.id.clone())
                    .await;
                self.emit_message_received(&trace_id, reply.to.clone(), reply.id.clone())
                    .await;
                Ok(reply)
            }
            Err(err) => {
                if matches!(err, SwarmError::TimeoutError(_)) {
                    self.emit_message_sent(&trace_id, from.clone(), to.clone(), message.id.clone())
                        .await;
                    self.emit_reply_timeout(&trace_id, from, to, message.id.clone(), timeout)
                        .await;
                }
                Err(err)
            }
        }
    }

    pub async fn multicast_agent_message(
        &self,
        trace_id: Option<TraceId>,
        from: AgentAddress,
        recipients: Vec<AgentAddress>,
        payload: Value,
    ) -> SwarmResult<Vec<crate::agent_comm::MessageId>> {
        let trace_id = trace_id.unwrap_or_else(Self::next_trace_id);
        let mut ids = Vec::new();
        for recipient in recipients {
            let message_id = self
                .send_agent_message(
                    Some(trace_id.clone()),
                    from.clone(),
                    recipient,
                    payload.clone(),
                )
                .await?;
            ids.push(message_id);
        }
        Ok(ids)
    }

    pub async fn broadcast_agent_message(
        &self,
        trace_id: Option<TraceId>,
        from: AgentAddress,
        payload: Value,
        include_sender: bool,
    ) -> SwarmResult<Vec<crate::agent_comm::MessageId>> {
        let sender_ref = from.agent_ref().clone();
        let recipients = self
            .agent_directory
            .all_refs()
            .into_iter()
            .filter(|agent| include_sender || *agent != sender_ref)
            .map(AgentAddress::local)
            .collect::<Vec<_>>();
        self.multicast_agent_message(trace_id, from, recipients, payload)
            .await
    }

    fn agent_matches_role(agent: &Agent, role: &TeamRole) -> bool {
        role.required_capabilities()
            .iter()
            .all(|capability| agent.has_capability(capability))
    }

    fn optional_capability_score(agent: &Agent, role: &TeamRole) -> usize {
        role.optional_capabilities()
            .iter()
            .filter(|capability| agent.has_capability(capability))
            .count()
    }

    async fn form_team_internal(
        &self,
        existing: Option<&AgentTeam>,
        roles: &[TeamRole],
        policy: TeamFormationPolicy,
    ) -> SwarmResult<AgentTeam> {
        if roles.is_empty() {
            return Err(SwarmError::ValidationError(
                "At least one team role is required".to_string(),
            ));
        }

        let load_snapshot = self
            .team_assignment_load
            .lock()
            .map_err(|_| SwarmError::Other("team_assignment_load lock poisoned".into()))?
            .clone();
        let mut projected_load = load_snapshot;
        let mut used = HashSet::new();
        let mut assignments = Vec::with_capacity(roles.len());
        let mut all_refs = self.agent_directory.all_refs();
        all_refs.sort_by(|a, b| a.as_str().cmp(b.as_str()));

        for role in roles {
            if policy.prefer_existing_assignments {
                if let Some(existing_team) = existing {
                    if let Some(existing_agent) = existing_team.agent_for_role(role.name()) {
                        if let Some(agent) = self.agent_directory.get(existing_agent) {
                            if Self::agent_matches_role(&agent, role)
                                && (policy.allow_agent_reuse || used.insert(existing_agent.clone()))
                            {
                                *projected_load.entry(existing_agent.clone()).or_default() += 1;
                                assignments.push(TeamAssignment::new(
                                    role.clone(),
                                    existing_agent.clone(),
                                ));
                                continue;
                            }
                        }
                    }
                }
            }

            let mut candidates = all_refs
                .iter()
                .filter(|agent_ref| policy.allow_agent_reuse || !used.contains(*agent_ref))
                .filter_map(|agent_ref| {
                    self.agent_directory
                        .get(agent_ref)
                        .map(|agent| (agent_ref.clone(), agent))
                })
                .filter(|(_, agent)| Self::agent_matches_role(agent, role))
                .collect::<Vec<_>>();

            candidates.sort_by(|(left_ref, left_agent), (right_ref, right_agent)| {
                let left_optional = Self::optional_capability_score(left_agent, role);
                let right_optional = Self::optional_capability_score(right_agent, role);
                right_optional
                    .cmp(&left_optional)
                    .then_with(|| {
                        projected_load
                            .get(left_ref)
                            .copied()
                            .unwrap_or(0)
                            .cmp(&projected_load.get(right_ref).copied().unwrap_or(0))
                    })
                    .then_with(|| left_ref.as_str().cmp(right_ref.as_str()))
            });

            let Some((selected_ref, _)) = candidates.into_iter().next() else {
                return Err(SwarmError::AgentNotFoundError(format!(
                    "No registered agent satisfies role '{}' with capabilities {:?}",
                    role.name(),
                    role.required_capabilities()
                )));
            };

            if !policy.allow_agent_reuse {
                used.insert(selected_ref.clone());
            }
            *projected_load.entry(selected_ref.clone()).or_default() += 1;
            assignments.push(TeamAssignment::new(role.clone(), selected_ref));
        }

        let team = AgentTeam::new(assignments)?;
        {
            let mut load = self
                .team_assignment_load
                .lock()
                .map_err(|_| SwarmError::Other("team_assignment_load lock poisoned".into()))?;
            *load = projected_load;
        }
        self.emit(AgentEvent::TeamFormed {
            trace_id: Self::next_trace_id(),
            team: team.clone(),
            timestamp: Utc::now(),
        })
        .await;
        Ok(team)
    }

    pub async fn form_team(&self, roles: &[TeamRole]) -> SwarmResult<AgentTeam> {
        self.form_team_internal(None, roles, TeamFormationPolicy::default())
            .await
    }

    pub async fn form_team_with_policy(
        &self,
        roles: &[TeamRole],
        policy: TeamFormationPolicy,
    ) -> SwarmResult<AgentTeam> {
        self.form_team_internal(None, roles, policy).await
    }

    pub async fn reconfigure_team(
        &self,
        existing: &AgentTeam,
        roles: &[TeamRole],
        policy: TeamFormationPolicy,
    ) -> SwarmResult<AgentTeam> {
        self.form_team_internal(Some(existing), roles, policy).await
    }

    pub async fn reach_consensus(
        &self,
        votes: &[TeamVote],
        strategy: ConsensusStrategy,
    ) -> SwarmResult<TeamDecision> {
        if votes.is_empty() {
            return Err(SwarmError::ValidationError(
                "At least one team vote is required".to_string(),
            ));
        }

        let mut tallies = HashMap::<String, u32>::new();
        for vote in votes {
            self.validate_local_agent(vote.agent())?;
            *tallies.entry(vote.option().to_string()).or_default() += vote.weight();
        }

        let total_votes = tallies.values().copied().sum::<u32>();
        let mut tallies = tallies
            .into_iter()
            .map(|(option, weight)| VoteTally::new(option, weight))
            .collect::<Vec<_>>();
        tallies.sort_by(|left, right| {
            right
                .weight()
                .cmp(&left.weight())
                .then_with(|| left.option().cmp(right.option()))
        });

        let selected = tallies.first().ok_or_else(|| {
            SwarmError::Other("Consensus tally was unexpectedly empty".to_string())
        })?;
        let unanimous = tallies.len() == 1;

        match strategy {
            ConsensusStrategy::Majority if selected.weight() * 2 <= total_votes => {
                return Err(SwarmError::Other(
                    "Majority consensus was not reached".to_string(),
                ));
            }
            ConsensusStrategy::Unanimous if !unanimous => {
                return Err(SwarmError::Other(
                    "Unanimous consensus was not reached".to_string(),
                ));
            }
            _ => {}
        }

        let decision = TeamDecision::new(
            strategy,
            selected.option().to_string(),
            total_votes,
            tallies,
            unanimous,
        );
        self.emit(AgentEvent::ConsensusReached {
            trace_id: Self::next_trace_id(),
            decision: decision.clone(),
            timestamp: Utc::now(),
        })
        .await;
        Ok(decision)
    }

    /// Emit an event to all registered subscribers.
    async fn emit(&self, event: AgentEvent) {
        if let Some(store) = &self.event_store {
            if let Err(err) = store.append_event(event.trace_id(), &event).await {
                tracing::warn!(
                    trace_id = event.trace_id(),
                    "event persistence failed (non-fatal): {}",
                    err
                );
            }
        }
        for sub in &self.subscribers {
            sub.on_event(&event).await;
        }
    }

    fn sanitize_text(&self, text: &str) -> (DataClassification, String) {
        classify_and_redact(
            text,
            &self.redaction_policy,
            self.redaction_threshold.clone(),
        )
    }

    fn sanitize_json_value(&self, value: &Value) -> (Option<DataClassification>, Value) {
        match value {
            Value::String(text) => {
                let (classification, redacted) = self.sanitize_text(text);
                let classification = match classification {
                    DataClassification::Public => None,
                    other => Some(other),
                };
                (classification, Value::String(redacted))
            }
            Value::Array(items) => {
                let mut highest = None;
                let mut sanitized = Vec::with_capacity(items.len());
                for item in items {
                    let (classification, value) = self.sanitize_json_value(item);
                    highest = max_classification(highest, classification);
                    sanitized.push(value);
                }
                (highest, Value::Array(sanitized))
            }
            Value::Object(map) => {
                let mut highest = None;
                let mut sanitized = serde_json::Map::with_capacity(map.len());
                for (key, item) in map {
                    let (classification, value) = self.sanitize_json_value(item);
                    highest = max_classification(highest, classification);
                    sanitized.insert(key.clone(), value);
                }
                (highest, Value::Object(sanitized))
            }
            _ => (None, value.clone()),
        }
    }

    fn get_tool_breaker(&self, tool_name: &str) -> SwarmResult<CircuitBreaker> {
        let mut guard = self
            .tool_breakers
            .lock()
            .map_err(|_| SwarmError::Other("tool_breakers lock poisoned".into()))?;
        Ok(guard
            .entry(tool_name.to_string())
            .or_insert_with(|| {
                CircuitBreaker::new(
                    format!("tool:{}", tool_name),
                    self.tool_breaker_settings.failure_threshold,
                    self.tool_breaker_settings.reset_secs,
                )
            })
            .clone())
    }

    async fn emit_guardrail_event(
        &self,
        trace_id: &TraceId,
        guardrail_type: &str,
        action: &str,
        details: &str,
        classification: Option<DataClassification>,
    ) {
        let (_, details) = self.sanitize_text(details);
        record_guardrail_triggered(guardrail_type);
        self.emit(AgentEvent::GuardrailTriggered {
            trace_id: trace_id.clone(),
            guardrail_type: guardrail_type.to_string(),
            action: action.to_string(),
            details,
            classification,
            timestamp: Utc::now(),
        })
        .await;
    }

    async fn emit_budget_event(&self, trace_id: &TraceId, exhausted: &BudgetExhausted) {
        let limit_type = match exhausted {
            BudgetExhausted::TokenBudget { .. } => "token_budget",
            BudgetExhausted::TokensPerRequest { .. } => "tokens_per_request",
            BudgetExhausted::WallTime { .. } => "wall_time",
            BudgetExhausted::ToolCallQuota { .. } => "tool_call_quota",
            BudgetExhausted::MaxDepth { .. } => "max_depth",
        };
        record_budget_exhausted(limit_type);
        self.emit(AgentEvent::BudgetExceeded {
            trace_id: trace_id.clone(),
            limit_type: limit_type.to_string(),
            details: exhausted.to_string(),
            timestamp: Utc::now(),
        })
        .await;
    }

    async fn emit_breaker_event(
        &self,
        trace_id: &TraceId,
        breaker: &CircuitBreaker,
        state: CircuitStateSnapshot,
        reason: Option<String>,
    ) {
        record_circuit_breaker_state(breaker.name(), &state.to_string());
        self.emit(AgentEvent::CircuitBreakerStateChanged {
            trace_id: trace_id.clone(),
            breaker_name: breaker.name().to_string(),
            state,
            reason,
            timestamp: Utc::now(),
        })
        .await;
    }

    async fn emit_escalation_event(
        &self,
        trace_id: &TraceId,
        trigger: EscalationTrigger,
        action: EscalationAction,
    ) {
        self.emit(AgentEvent::EscalationTriggered {
            trace_id: trace_id.clone(),
            trigger,
            action,
            timestamp: Utc::now(),
        })
        .await;
    }

    async fn create_session_if_configured(&self, trace_id: &TraceId, agent_name: &str) {
        let Some(store) = &self.session_store else {
            return;
        };
        if let Err(err) = store
            .create_session(trace_id.as_str(), agent_name, trace_id.as_str())
            .await
        {
            tracing::warn!(
                session_id = trace_id.as_str(),
                "session creation failed (non-fatal): {}",
                err
            );
        }
    }

    async fn store_messages_if_configured(&self, trace_id: &TraceId, messages: &[Message]) {
        let Some(store) = &self.session_store else {
            return;
        };
        if let Err(err) = store.store_messages(trace_id.as_str(), messages).await {
            tracing::warn!(
                session_id = trace_id.as_str(),
                "message persistence failed (non-fatal): {}",
                err
            );
        }
    }

    async fn complete_session_if_configured(&self, trace_id: &TraceId, outcome: &str) {
        let Some(store) = &self.session_store else {
            return;
        };
        if let Err(err) = store.complete_session(trace_id.as_str(), outcome).await {
            tracing::warn!(
                session_id = trace_id.as_str(),
                "session completion failed (non-fatal): {}",
                err
            );
        }
    }

    async fn persist_memory_hook(&self, trace_id: &TraceId, key: &str, value: &str, source: &str) {
        let Some(store) = &self.memory_store else {
            return;
        };
        let (classification, sanitized) = self.sanitize_text(value);
        if let Err(err) = store
            .persist_memory(trace_id.as_str(), key, &sanitized)
            .await
        {
            tracing::warn!(
                session_id = trace_id.as_str(),
                memory_key = key,
                "memory persistence failed (non-fatal): {}",
                err
            );
            return;
        }
        self.emit(AgentEvent::MemoryPersisted {
            trace_id: trace_id.clone(),
            key: key.to_string(),
            source: source.to_string(),
            classification,
            timestamp: Utc::now(),
        })
        .await;
    }

    async fn enforce_content_policy(
        &self,
        trace_id: &TraceId,
        text: &str,
        context: &str,
    ) -> SwarmResult<()> {
        match self.content_policy.check_text(text, context).await {
            PolicyResult::Allow => Ok(()),
            PolicyResult::Warn(message) => {
                self.emit_guardrail_event(
                    trace_id,
                    "content_policy",
                    "warn",
                    &message,
                    Some(DataClassification::Restricted),
                )
                .await;
                Ok(())
            }
            PolicyResult::Block(message) => {
                self.emit_guardrail_event(
                    trace_id,
                    "content_policy",
                    "block",
                    &message,
                    Some(DataClassification::Restricted),
                )
                .await;
                Err(SwarmError::Other(message))
            }
        }
    }

    async fn apply_injection_policy(
        &self,
        trace_id: &TraceId,
        messages: &mut [Message],
    ) -> SwarmResult<()> {
        for message in messages.iter_mut() {
            let Some(content) = message.content().map(str::to_string) else {
                continue;
            };
            match check_injection_with_policy(&content, &self.injection_policy) {
                InjectionOutcome::Clean => {}
                InjectionOutcome::Warned { patterns } => {
                    self.emit_guardrail_event(
                        trace_id,
                        "prompt_injection",
                        "warn",
                        &format!("patterns={:?}", patterns),
                        Some(DataClassification::Restricted),
                    )
                    .await;
                }
                InjectionOutcome::Sanitized {
                    patterns,
                    sanitized,
                } => {
                    self.emit_guardrail_event(
                        trace_id,
                        "prompt_injection",
                        "sanitize",
                        &format!("patterns={:?}", patterns),
                        Some(DataClassification::Restricted),
                    )
                    .await;
                    *message = Message::from_parts_unchecked(
                        message.role(),
                        Some(sanitized),
                        message.name().map(str::to_string),
                        message.function_call().cloned(),
                    );
                }
                InjectionOutcome::Rejected { patterns } => {
                    self.emit_guardrail_event(
                        trace_id,
                        "prompt_injection",
                        "reject",
                        &format!("patterns={:?}", patterns),
                        Some(DataClassification::Restricted),
                    )
                    .await;
                    return Err(SwarmError::ValidationError(format!(
                        "Prompt injection rejected by policy: {:?}",
                        patterns
                    )));
                }
            }
        }
        Ok(())
    }

    async fn check_budget(&self, trace_id: &TraceId, budget: &BudgetEnforcer) -> SwarmResult<()> {
        if let Err(exhausted) = budget.check() {
            self.emit_budget_event(trace_id, &exhausted).await;
            return Err(exhausted.into());
        }
        Ok(())
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
        // Defense-in-depth: preflight (validate_api_request) is the authoritative check.
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

            if agent.tool_call_execution().is_parallel() {
                request_body["parallel_tool_calls"] = json!(true);
            }

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

            // Line buffer: TCP chunks can split SSE `data:` lines across boundaries.
            let mut line_buf = String::new();
            // Delta accumulators for the single choice we're building.
            let mut content_buf = String::new();
            let mut fc_name = String::new();
            let mut fc_args = String::new();
            let mut finish_reason: Option<FinishReason> = None;
            // Accumulator for multi-tool-call streaming deltas (OpenAI tool_calls API).
            let mut tc_acc_msg =
                Message::from_parts_unchecked(MessageRole::Assistant, None, None, None);

            'sse: while let Some(chunk_result) = stream.next().await {
                let data = chunk_result.map_err(|e| {
                    SwarmError::StreamError(format!("Error reading streaming response: {}", e))
                })?;
                line_buf.push_str(&String::from_utf8_lossy(&data));

                // Process every complete line (terminated by \n).
                while let Some(newline_pos) = line_buf.find('\n') {
                    let line = line_buf[..newline_pos].trim_end_matches('\r').to_string();
                    line_buf.drain(..=newline_pos);

                    if let Some(json_str) = line.strip_prefix("data: ") {
                        let json_str = json_str.trim();
                        if json_str == "[DONE]" {
                            break 'sse;
                        }
                        let chunk: Value = serde_json::from_str(json_str).map_err(|e| {
                            SwarmError::DeserializationError(format!(
                                "Failed to parse SSE chunk: {}",
                                e
                            ))
                        })?;
                        if let Some(choices) = chunk["choices"].as_array() {
                            for choice in choices {
                                let delta = &choice["delta"];
                                if let Some(text) = delta["content"].as_str() {
                                    content_buf.push_str(text);
                                }
                                if let Some(fc) = delta.get("function_call") {
                                    if let Some(name) = fc["name"].as_str() {
                                        fc_name.push_str(name);
                                    }
                                    if let Some(args) = fc["arguments"].as_str() {
                                        fc_args.push_str(args);
                                    }
                                }
                                if let Some(tc_arr) =
                                    delta.get("tool_calls").and_then(|v| v.as_array())
                                {
                                    for tc_delta in tc_arr {
                                        let index =
                                            tc_delta["index"].as_u64().unwrap_or(0) as usize;
                                        tc_acc_msg.merge_tool_call_delta(index, tc_delta);
                                    }
                                }
                                if let Some(fr) = choice["finish_reason"].as_str() {
                                    finish_reason = Some(match fr {
                                        "stop" => FinishReason::Stop,
                                        "length" => FinishReason::Length,
                                        "content_filter" => FinishReason::ContentFilter,
                                        "tool_calls" => FinishReason::ToolCalls,
                                        "function_call" => FinishReason::FunctionCall,
                                        other => FinishReason::Unknown(other.to_string()),
                                    });
                                }
                            }
                        }
                    }
                }
            }

            // Assemble the fully merged message from accumulated deltas.
            tc_acc_msg.finalize_tool_calls();
            let merged_message = if let Some(tool_calls) = tc_acc_msg.tool_calls() {
                // Multi-tool-call streaming response.
                Message::assistant_tool_calls(tool_calls.to_vec()).map_err(|e| {
                    SwarmError::DeserializationError(format!(
                        "Failed to build tool_calls message: {}",
                        e
                    ))
                })?
            } else {
                let merged_fc = if !fc_name.is_empty() || !fc_args.is_empty() {
                    Some(FunctionCall::from_parts_unchecked(fc_name, fc_args))
                } else {
                    None
                };
                let merged_content = if !content_buf.is_empty() {
                    Some(content_buf)
                } else {
                    None
                };
                Message::from_parts_unchecked(
                    MessageRole::Assistant,
                    merged_content,
                    None,
                    merged_fc,
                )
            };
            let mut full_response = ChatCompletionResponse::accumulator();
            full_response.extend_choices(vec![Choice {
                index: 0,
                message: merged_message,
                finish_reason,
            }]);
            Ok(full_response)
        } else {
            // Non-streaming path: delegate to provider, then map response via JSON round-trip.
            let functions: Vec<Value> = agent
                .functions
                .iter()
                .map(function_to_json)
                .collect::<SwarmResult<Vec<Value>>>()?;
            let function_call_policy = agent.function_call().to_wire_value().map(|v| json!(v));

            let mut request = CompletionRequest::new(model, messages);
            if !functions.is_empty() {
                request = request.with_functions(functions, function_call_policy);
            }
            if agent.tool_call_execution().is_parallel() {
                request = request.with_parallel_tool_calls(true);
            }

            let provider_response = self.provider.complete(request).await?;
            debug_print(
                debug,
                &format!("Provider Response: {:?}", provider_response),
            );

            let mut json_val = serde_json::to_value(&provider_response).map_err(|e| {
                SwarmError::DeserializationError(format!(
                    "Failed to serialize provider response: {}",
                    e
                ))
            })?;

            // Map tool_calls → function_call when there is exactly one tool call (backward-compat).
            // For multiple tool calls, leave the array intact so MessageDto deserializes it into
            // Message::tool_calls, which is dispatched by the parallel/serial execution branch.
            if let Some(choices) = json_val["choices"].as_array_mut() {
                for choice in choices.iter_mut() {
                    let tc_count = choice["message"]["tool_calls"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0);
                    if tc_count == 1 {
                        // Single-call: promote to function_call and remove the array (legacy path).
                        let first = choice["message"]["tool_calls"][0].clone();
                        let name = first["function"]["name"].clone();
                        let args = first["function"]["arguments"].clone();
                        let args_str = if args.is_string() {
                            args.clone()
                        } else {
                            Value::String(
                                serde_json::to_string(&args)
                                    .map_err(|e| SwarmError::DeserializationError(e.to_string()))?,
                            )
                        };
                        choice["message"]["function_call"] =
                            json!({"name": name, "arguments": args_str});
                        choice["message"]
                            .as_object_mut()
                            .map(|m| m.remove("tool_calls"));
                    }
                    // tc_count > 1: leave tool_calls in place for the multi-call dispatch path.
                    // tc_count == 0: no-op.
                }
            }

            serde_json::from_value(json_val)
                .map_err(|e| SwarmError::DeserializationError(e.to_string()))
        }
    }

    /// Asynchronously handles a function call from an agent.
    pub async fn handle_function_call(
        &self,
        function_call: &FunctionCall,
        functions: &[AgentFunction],
        context_variables: ContextVariables,
        debug: bool,
    ) -> SwarmResult<Response> {
        if function_call.name().trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "Function call name cannot be empty.".to_string(),
            ));
        }

        let mut function_map = HashMap::new();
        for func in functions {
            function_map.insert(func.name().to_string(), func.clone());
        }

        let mut response = Response {
            messages: Vec::new(),
            agent: None,
            context_variables: HashMap::new(),
            termination_reason: None,
            tokens_used: 0,
        };

        if let Some(func) = function_map.get(function_call.name()) {
            let invocation_args = InvocationArgs::from_json_str(function_call.arguments())
                .map_err(|error| SwarmError::ValidationError(error.to_string()))?;
            invocation_args
                .validate_against_schema(func.parameters_schema())
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
            if func.accepts_context_variables() {
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

    /// Executes multiple tool calls serially, threading context from each call to the next.
    async fn handle_tool_calls_serial(
        &self,
        tool_calls: &[ToolCall],
        functions: &[AgentFunction],
        context_variables: &ContextVariables,
        debug: bool,
    ) -> Vec<ToolCallOutcome> {
        let mut output = Vec::with_capacity(tool_calls.len());
        let mut running_ctx = context_variables.clone();
        for tc in tool_calls {
            let response = self
                .handle_function_call(tc.function(), functions, running_ctx.clone(), debug)
                .await;
            match response {
                Ok(response) => {
                    running_ctx.extend(response.context_variables.clone());
                    output.push(ToolCallOutcome {
                        tool_call: tc.clone(),
                        response: Ok(response),
                    });
                }
                Err(err) => {
                    output.push(ToolCallOutcome {
                        tool_call: tc.clone(),
                        response: Err(err),
                    });
                    break;
                }
            }
        }
        output
    }

    /// Executes multiple tool calls in parallel. Each call receives a clone of the current
    /// context; results are merged in input order (last-writer-wins for conflicting keys).
    async fn handle_tool_calls_parallel(
        &self,
        tool_calls: &[ToolCall],
        functions: &[AgentFunction],
        context_variables: &ContextVariables,
        debug: bool,
    ) -> Vec<ToolCallOutcome> {
        let futs: Vec<_> = tool_calls
            .iter()
            .map(|tc| {
                let ctx = context_variables.clone();
                let fc = tc.function().clone();
                let fns = functions.to_vec();
                async move { self.handle_function_call(&fc, &fns, ctx, debug).await }
            })
            .collect();
        let results = futures::future::join_all(futs).await;
        tool_calls
            .iter()
            .zip(results)
            .map(|(tc, response)| ToolCallOutcome {
                tool_call: tc.clone(),
                response,
            })
            .collect()
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

    async fn persist_iteration_state(&self, trace_id: &TraceId, state: &RunState) {
        self.store_messages_if_configured(trace_id, &state.history)
            .await;
        self.save_checkpoint_if_configured(
            trace_id.as_str(),
            state.agent.name(),
            &state.history,
            &state.context_variables,
            state.iterations,
            state.total_tokens,
        )
        .await;
    }

    async fn apply_escalation_trigger(
        &self,
        state: &mut RunState,
        exec: &mut ExecutionContext<'_>,
        trigger: EscalationTrigger,
    ) -> SwarmResult<Option<crate::phase::TerminationReason>> {
        let action = exec.escalation.config().action.clone();
        self.emit_escalation_event(exec.trace_id, trigger.clone(), action.clone())
            .await;
        match action {
            EscalationAction::Stop => Ok(Some(crate::phase::TerminationReason::DoomLoopDetected)),
            EscalationAction::InjectWarning => {
                let warning = EscalationDetector::warning_message(&trigger);
                state.history.push(Message::assistant(warning.clone())?);
                self.persist_memory_hook(
                    exec.trace_id,
                    &format!("warning:{}", state.iterations),
                    &warning,
                    "escalation_warning",
                )
                .await;
                Ok(None)
            }
            EscalationAction::HumanReviewEvent => Ok(None),
        }
    }

    /// Executes a single round of conversation with the agent.
    async fn single_execution(
        &self,
        state: &mut RunState,
        exec: &mut ExecutionContext<'_>,
    ) -> SwarmResult<Response> {
        self.check_budget(exec.trace_id, exec.budget).await?;
        exec.budget.increment_iterations();
        state.iterations = exec.budget.iterations;
        record_iteration(state.agent.name());

        let model = exec
            .options
            .model_override
            .as_deref()
            .unwrap_or(state.agent.model())
            .to_string();
        // Heuristic: ~4 bytes per token. Actual counts come from the API response.
        let prompt_tokens: u32 = state
            .history
            .iter()
            .map(|m| m.content().map(|c| (c.len() / 4) as u32).unwrap_or(4))
            .sum();
        if let Some(limit) = self.config.runtime_limits().max_tokens_per_request {
            if prompt_tokens > limit {
                let exhausted = BudgetExhausted::TokensPerRequest {
                    used: prompt_tokens,
                    limit,
                };
                self.emit_budget_event(exec.trace_id, &exhausted).await;
                return Err(exhausted.into());
            }
        }

        self.emit(AgentEvent::LlmRequest {
            trace_id: exec.trace_id.clone(),
            model: model.clone(),
            prompt_tokens: prompt_tokens as usize,
            timestamp: Utc::now(),
        })
        .await;

        let start = Instant::now();
        let strategy = self.config.api_settings().retry_strategy().clone();
        let completion = {
            let mut delay = strategy.initial_delay();
            let mut last_err: Option<SwarmError> = None;
            let mut result = None;

            for attempt in 0..=strategy.max_retries() {
                let provider_before = self.provider_breaker.state_snapshot();
                let provider_open = self.provider_breaker.is_open();
                let provider_after = self.provider_breaker.state_snapshot();
                if provider_after != provider_before {
                    self.emit_breaker_event(
                        exec.trace_id,
                        &self.provider_breaker,
                        provider_after.clone(),
                        None,
                    )
                    .await;
                }
                if provider_open {
                    return Err(SwarmError::Other(format!(
                        "Provider circuit breaker '{}' is open",
                        self.provider_breaker.name()
                    )));
                }

                match self
                    .get_chat_completion(
                        &state.agent,
                        &state.history,
                        &state.context_variables,
                        exec.options.model_override.clone(),
                        exec.options.stream,
                        exec.options.debug,
                    )
                    .await
                {
                    Ok(completion) => {
                        let provider_before = self.provider_breaker.state_snapshot();
                        self.provider_breaker.record_success();
                        let provider_after = self.provider_breaker.state_snapshot();
                        if provider_after != provider_before {
                            self.emit_breaker_event(
                                exec.trace_id,
                                &self.provider_breaker,
                                provider_after,
                                None,
                            )
                            .await;
                        }
                        result = Some(completion);
                        break;
                    }
                    Err(err) if attempt < strategy.max_retries() && err.is_retriable() => {
                        let provider_before = self.provider_breaker.state_snapshot();
                        let reason = err.to_string();
                        let provider_after = self.provider_breaker.record_failure();
                        if provider_after != provider_before {
                            self.emit_breaker_event(
                                exec.trace_id,
                                &self.provider_breaker,
                                provider_after,
                                Some(reason.clone()),
                            )
                            .await;
                        }
                        tracing::warn!(
                            "Retryable LLM error on attempt {}/{}, retrying in {}ms: {}",
                            attempt + 1,
                            strategy.max_retries(),
                            delay.as_millis(),
                            err
                        );
                        tokio::time::sleep(delay).await;
                        let next_ms =
                            (delay.as_millis() as f64 * strategy.backoff_factor() as f64) as u64;
                        delay = Duration::from_millis(
                            next_ms.min(strategy.max_delay().as_millis() as u64),
                        );
                        last_err = Some(err);
                    }
                    Err(err) => {
                        let provider_before = self.provider_breaker.state_snapshot();
                        let reason = err.to_string();
                        let provider_after = self.provider_breaker.record_failure();
                        if provider_after != provider_before {
                            self.emit_breaker_event(
                                exec.trace_id,
                                &self.provider_breaker,
                                provider_after,
                                Some(reason),
                            )
                            .await;
                        }
                        last_err = Some(err);
                        break;
                    }
                }
            }

            result.ok_or_else(|| {
                last_err
                    .unwrap_or_else(|| SwarmError::Other("Retry attempts exhausted".to_string()))
            })?
        };
        let latency_ms = start.elapsed().as_millis() as u64;

        if completion.choices().is_empty() {
            return Err(SwarmError::ApiError(
                "No choices returned from the model".to_string(),
            ));
        }

        let completion_tokens = completion
            .usage()
            .map(|usage| usage.completion_tokens)
            .unwrap_or_else(|| {
                completion
                    .choices()
                    .first()
                    .and_then(|choice| choice.message.content().map(|text| (text.len() / 4) as u32))
                    .unwrap_or(0)
            });
        let tokens_used = completion
            .usage()
            .map(|usage| usage.total_tokens)
            .unwrap_or(prompt_tokens.saturating_add(completion_tokens));

        exec.budget.add_tokens(tokens_used);
        state.total_tokens = exec.budget.total_tokens;
        self.check_budget(exec.trace_id, exec.budget).await?;
        record_llm_latency(latency_ms as f64, &model);
        record_token_usage(tokens_used as u64, &model);

        self.emit(AgentEvent::LlmResponse {
            trace_id: exec.trace_id.clone(),
            model,
            completion_tokens: completion_tokens as usize,
            latency_ms,
            timestamp: Utc::now(),
        })
        .await;

        let message = completion.choices()[0].message.clone();
        if let Some(content) = message.content() {
            self.enforce_content_policy(exec.trace_id, content, "llm_response")
                .await?;
        }
        if !state.agent.expected_response_fields().is_empty() {
            let content = message.content().ok_or_else(|| {
                SwarmError::ValidationError(
                    "Expected a structured JSON response but assistant content was empty"
                        .to_string(),
                )
            })?;
            let structured: Value = serde_json::from_str(content).map_err(|error| {
                SwarmError::ValidationError(format!("Expected structured JSON response: {}", error))
            })?;
            let expected_fields = state
                .agent
                .expected_response_fields()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            verify_structured_response(&structured, &expected_fields)?;
        }

        state.history.push(message.clone());
        if let Some(content) = message.content() {
            self.persist_memory_hook(
                exec.trace_id,
                &format!("assistant:{}:response", state.iterations),
                content,
                "assistant_response",
            )
            .await;
        }

        let mut termination_reason = None;
        if let Some(function_call) = message.function_call() {
            let known_tools = state
                .agent
                .functions()
                .iter()
                .map(|function| function.name())
                .collect::<Vec<_>>();
            let breaker = self.get_tool_breaker(function_call.name())?;

            let tool_before = breaker.state_snapshot();
            let tool_open = breaker.is_open();
            let tool_after = breaker.state_snapshot();
            if tool_after != tool_before {
                self.emit_breaker_event(exec.trace_id, &breaker, tool_after.clone(), None)
                    .await;
            }
            if tool_open {
                return Err(SwarmError::Other(format!(
                    "Tool circuit breaker '{}' is open",
                    breaker.name()
                )));
            }

            let arguments = serde_json::from_str(function_call.arguments()).unwrap_or(Value::Null);
            let (_, sanitized_arguments) = self.sanitize_json_value(&arguments);
            self.emit(AgentEvent::ToolCall {
                trace_id: exec.trace_id.clone(),
                tool_name: function_call.name().to_string(),
                arguments: sanitized_arguments,
                timestamp: Utc::now(),
            })
            .await;

            self.check_budget(exec.trace_id, exec.budget).await?;
            let tool_start = Instant::now();
            let func_response = self
                .handle_function_call(
                    function_call,
                    state.agent.functions(),
                    state.context_variables.clone(),
                    exec.options.debug,
                )
                .await;
            let tool_duration_ms = tool_start.elapsed().as_millis() as u64;
            exec.budget.increment_tool_calls();

            match func_response {
                Ok(func_response) => {
                    let tool_result_content = func_response
                        .messages
                        .first()
                        .and_then(|message| {
                            message
                                .content()
                                .map(|content| Value::String(content.to_string()))
                        })
                        .unwrap_or(Value::Null);
                    let tool_success = !tool_result_content
                        .as_str()
                        .map(|content| content.starts_with("Error:"))
                        .unwrap_or(false);

                    if tool_success {
                        let tool_before = breaker.state_snapshot();
                        breaker.record_success();
                        let tool_after = breaker.state_snapshot();
                        if tool_after != tool_before {
                            self.emit_breaker_event(exec.trace_id, &breaker, tool_after, None)
                                .await;
                        }
                    } else {
                        let tool_before = breaker.state_snapshot();
                        let tool_after = breaker.record_failure();
                        if tool_after != tool_before {
                            self.emit_breaker_event(
                                exec.trace_id,
                                &breaker,
                                tool_after,
                                Some("tool returned an error response".to_string()),
                            )
                            .await;
                        }
                    }

                    let (classification, sanitized_result) =
                        self.sanitize_json_value(&tool_result_content);
                    self.emit(AgentEvent::ToolResult {
                        trace_id: exec.trace_id.clone(),
                        tool_name: function_call.name().to_string(),
                        result: sanitized_result,
                        success: tool_success,
                        duration_ms: tool_duration_ms,
                        timestamp: Utc::now(),
                    })
                    .await;
                    record_tool_call(function_call.name(), tool_duration_ms as f64, tool_success);

                    if let Some(content) = tool_result_content.as_str() {
                        self.enforce_content_policy(exec.trace_id, content, "tool_result")
                            .await?;
                        self.persist_memory_hook(
                            exec.trace_id,
                            &format!("tool:{}:{}", function_call.name(), state.iterations),
                            content,
                            "tool_result",
                        )
                        .await;
                    } else if let Some(classification) = classification {
                        self.emit_guardrail_event(
                            exec.trace_id,
                            "tool_result_redaction",
                            "redact",
                            "non-string tool result redacted for audit storage",
                            Some(classification),
                        )
                        .await;
                    }

                    if let Some(trigger) = exec.escalation.record_tool_call(
                        function_call.name(),
                        tool_success,
                        &known_tools,
                        function_call.arguments(),
                    ) {
                        if let Some(reason) =
                            self.apply_escalation_trigger(state, exec, trigger).await?
                        {
                            termination_reason = Some(reason);
                        }
                    }

                    state.history.extend(func_response.messages);
                    state
                        .context_variables
                        .extend(func_response.context_variables);
                    if let Some(agent) = func_response.agent {
                        exec.budget.increment_depth();
                        self.check_budget(exec.trace_id, exec.budget).await?;
                        state.agent = agent;
                    }
                    if let Some(reason) = func_response.termination_reason {
                        termination_reason = Some(reason);
                    }
                }
                Err(err) => {
                    let tool_before = breaker.state_snapshot();
                    let tool_after = breaker.record_failure();
                    if tool_after != tool_before {
                        self.emit_breaker_event(
                            exec.trace_id,
                            &breaker,
                            tool_after,
                            Some(err.to_string()),
                        )
                        .await;
                    }
                    self.emit(AgentEvent::ToolResult {
                        trace_id: exec.trace_id.clone(),
                        tool_name: function_call.name().to_string(),
                        result: Value::String(err.to_string()),
                        success: false,
                        duration_ms: tool_duration_ms,
                        timestamp: Utc::now(),
                    })
                    .await;
                    record_tool_call(function_call.name(), tool_duration_ms as f64, false);
                    if let Some(trigger) = exec.escalation.record_tool_call(
                        function_call.name(),
                        false,
                        &known_tools,
                        function_call.arguments(),
                    ) {
                        if let Some(reason) =
                            self.apply_escalation_trigger(state, exec, trigger).await?
                        {
                            return Ok(Response {
                                messages: vec![message],
                                agent: Some(state.agent.clone()),
                                context_variables: state.context_variables.clone(),
                                termination_reason: Some(reason),
                                tokens_used,
                            });
                        }
                    }
                    return Err(err);
                }
            }
        } else if let Some(tool_calls) = message.tool_calls() {
            if !tool_calls.is_empty() {
                // Snapshot immutable state before any await point or mutation of `state`.
                // Cloning here avoids holding borrows of state.agent / state.context_variables
                // across await points, which the borrow checker would reject.
                let functions_snapshot = state.agent.functions().to_vec();
                let ctx_snapshot = state.context_variables.clone();
                let execution_mode = state.agent.tool_call_execution();
                // known_tools borrows from functions_snapshot (local), not state.agent.
                let known_tools: Vec<&str> = functions_snapshot.iter().map(|f| f.name()).collect();

                // Pre-execution: circuit breaker check per call
                for tc in tool_calls {
                    let breaker = self.get_tool_breaker(tc.function().name())?;
                    let snap_before = breaker.state_snapshot();
                    let is_open = breaker.is_open();
                    let snap_after = breaker.state_snapshot();
                    if snap_after != snap_before {
                        self.emit_breaker_event(exec.trace_id, &breaker, snap_after.clone(), None)
                            .await;
                    }
                    if is_open {
                        return Err(SwarmError::Other(format!(
                            "Tool circuit breaker '{}' is open",
                            breaker.name()
                        )));
                    }
                }

                // Emit ToolCall events
                for tc in tool_calls {
                    let arguments =
                        serde_json::from_str(tc.function().arguments()).unwrap_or(Value::Null);
                    let (_, sanitized_arguments) = self.sanitize_json_value(&arguments);
                    self.emit(AgentEvent::ToolCall {
                        trace_id: exec.trace_id.clone(),
                        tool_name: tc.function().name().to_string(),
                        arguments: sanitized_arguments,
                        timestamp: Utc::now(),
                    })
                    .await;
                }

                self.check_budget(exec.trace_id, exec.budget).await?;

                let tool_start = Instant::now();
                let batch_results = match execution_mode {
                    ToolCallExecution::Parallel => {
                        self.handle_tool_calls_parallel(
                            tool_calls,
                            &functions_snapshot,
                            &ctx_snapshot,
                            exec.options.debug,
                        )
                        .await
                    }
                    ToolCallExecution::Serial => {
                        self.handle_tool_calls_serial(
                            tool_calls,
                            &functions_snapshot,
                            &ctx_snapshot,
                            exec.options.debug,
                        )
                        .await
                    }
                };
                let tool_duration_ms = tool_start.elapsed().as_millis() as u64;
                let mut batch_error = None;

                for outcome in batch_results {
                    let tc = outcome.tool_call;
                    exec.budget.increment_tool_calls();
                    match outcome.response {
                        Ok(func_response) => {
                            let tool_result_content = func_response
                                .messages
                                .first()
                                .and_then(|m| m.content().map(|c| Value::String(c.to_string())))
                                .unwrap_or(Value::Null);
                            let tool_success = !tool_result_content
                                .as_str()
                                .map(|c| c.starts_with("Error:"))
                                .unwrap_or(false);

                            let breaker = self.get_tool_breaker(tc.function().name())?;
                            if tool_success {
                                let snap_before = breaker.state_snapshot();
                                breaker.record_success();
                                let snap_after = breaker.state_snapshot();
                                if snap_after != snap_before {
                                    self.emit_breaker_event(
                                        exec.trace_id,
                                        &breaker,
                                        snap_after,
                                        None,
                                    )
                                    .await;
                                }
                            } else {
                                let snap_before = breaker.state_snapshot();
                                let snap_after = breaker.record_failure();
                                if snap_after != snap_before {
                                    self.emit_breaker_event(
                                        exec.trace_id,
                                        &breaker,
                                        snap_after,
                                        Some("tool returned an error response".to_string()),
                                    )
                                    .await;
                                }
                            }

                            let (classification, sanitized_result) =
                                self.sanitize_json_value(&tool_result_content);
                            self.emit(AgentEvent::ToolResult {
                                trace_id: exec.trace_id.clone(),
                                tool_name: tc.function().name().to_string(),
                                result: sanitized_result,
                                success: tool_success,
                                duration_ms: tool_duration_ms,
                                timestamp: Utc::now(),
                            })
                            .await;
                            record_tool_call(
                                tc.function().name(),
                                tool_duration_ms as f64,
                                tool_success,
                            );

                            if let Some(content) = tool_result_content.as_str() {
                                self.enforce_content_policy(exec.trace_id, content, "tool_result")
                                    .await?;
                                self.persist_memory_hook(
                                    exec.trace_id,
                                    &format!("tool:{}:{}", tc.function().name(), state.iterations),
                                    content,
                                    "tool_result",
                                )
                                .await;
                            } else if let Some(cls) = classification {
                                self.emit_guardrail_event(
                                    exec.trace_id,
                                    "tool_result_redaction",
                                    "redact",
                                    "non-string tool result redacted for audit storage",
                                    Some(cls),
                                )
                                .await;
                            }

                            if let Some(trigger) = exec.escalation.record_tool_call(
                                tc.function().name(),
                                tool_success,
                                &known_tools,
                                tc.function().arguments(),
                            ) {
                                if let Some(reason) =
                                    self.apply_escalation_trigger(state, exec, trigger).await?
                                {
                                    termination_reason = Some(reason);
                                }
                            }

                            // Push tool result to history with tool_call_id linkage.
                            let result_str = tool_result_content
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .unwrap_or("null")
                                .to_string();
                            state
                                .history
                                .push(Message::tool_result(tc.id(), result_str)?);
                            state
                                .context_variables
                                .extend(func_response.context_variables);
                            if let Some(agent) = func_response.agent {
                                exec.budget.increment_depth();
                                self.check_budget(exec.trace_id, exec.budget).await?;
                                state.agent = agent;
                            }
                            if let Some(reason) = func_response.termination_reason {
                                termination_reason = Some(reason);
                            }
                        }
                        Err(err) => {
                            let err_text = err.to_string();
                            let breaker = self.get_tool_breaker(tc.function().name())?;
                            let snap_before = breaker.state_snapshot();
                            let snap_after = breaker.record_failure();
                            if snap_after != snap_before {
                                self.emit_breaker_event(
                                    exec.trace_id,
                                    &breaker,
                                    snap_after,
                                    Some(err_text.clone()),
                                )
                                .await;
                            }
                            self.emit(AgentEvent::ToolResult {
                                trace_id: exec.trace_id.clone(),
                                tool_name: tc.function().name().to_string(),
                                result: Value::String(err_text.clone()),
                                success: false,
                                duration_ms: tool_duration_ms,
                                timestamp: Utc::now(),
                            })
                            .await;
                            record_tool_call(tc.function().name(), tool_duration_ms as f64, false);
                            if let Some(trigger) = exec.escalation.record_tool_call(
                                tc.function().name(),
                                false,
                                &known_tools,
                                tc.function().arguments(),
                            ) {
                                if let Some(reason) =
                                    self.apply_escalation_trigger(state, exec, trigger).await?
                                {
                                    termination_reason = Some(reason);
                                }
                            }
                            if batch_error.is_none() {
                                batch_error = Some(err);
                            }
                        }
                    }
                }
                if let Some(err) = batch_error {
                    if termination_reason.is_none() {
                        return Err(err);
                    }
                }
            }
        }

        Ok(Response {
            messages: vec![message],
            agent: Some(state.agent.clone()),
            context_variables: state.context_variables.clone(),
            termination_reason,
            tokens_used,
        })
    }

    /// Executes a step based on the provided XML-defined step.
    async fn execute_step(
        &self,
        state: &mut RunState,
        step: &Step,
        exec: &mut ExecutionContext<'_>,
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

        debug_print(
            exec.options.debug,
            &format!("Executing Step {}", step.number),
        );

        if let Some(agent_name) = &step.agent {
            debug_print(
                exec.options.debug,
                &format!("Switching to agent: {}", agent_name),
            );
            state.agent = self.get_agent_by_name(agent_name)?;
            exec.budget.increment_depth();
            self.check_budget(exec.trace_id, exec.budget).await?;
        }

        match step.action {
            crate::types::StepAction::RunOnce => {
                state.history.push(Message::user(step.prompt.clone())?);
                let response = self.single_execution(state, exec).await?;
                self.persist_iteration_state(exec.trace_id, state).await;
                Ok(response)
            }
            crate::types::StepAction::Loop => {
                let mut loop_iterations = 0usize;
                let termination_reason = loop {
                    if loop_iterations >= exec.options.max_turns {
                        return Err(SwarmError::MaxIterationsError {
                            max: exec.options.max_turns,
                            actual: loop_iterations,
                        });
                    }
                    loop_iterations += 1;
                    state.history.push(Message::user(step.prompt.clone())?);
                    let response = self.single_execution(state, exec).await?;
                    self.persist_iteration_state(exec.trace_id, state).await;
                    if let Some(reason) = response.termination_reason {
                        debug_print(exec.options.debug, &format!("Loop terminated: {}", reason));
                        break Some(reason);
                    }
                };
                Ok(Response {
                    messages: state.history.clone(),
                    agent: Some(state.agent.clone()),
                    context_variables: state.context_variables.clone(),
                    termination_reason,
                    tokens_used: state.total_tokens,
                })
            }
        }
    }

    /// Executes a multi-turn conversation with the AI agent.
    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        &self,
        mut agent: Agent,
        messages: Vec<Message>,
        context_variables: ContextVariables,
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

        let trace_id = TraceId::from(uuid::Uuid::new_v4().to_string());
        let options = RunOptions {
            model_override,
            stream,
            debug,
            max_turns,
        };

        self.create_session_if_configured(&trace_id, agent.name())
            .await;
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

        // If the entire instructions block was XML steps, fall back to a minimal
        // system prompt rather than producing an empty string that fails validation.
        let effective_instructions =
            if instructions_without_xml.trim().is_empty() && !steps.steps.is_empty() {
                "You are a helpful assistant.".to_string()
            } else {
                instructions_without_xml
            };
        agent.instructions = Instructions::Text(effective_instructions);
        let mut state = RunState {
            agent,
            history: messages,
            context_variables,
            iterations: 0,
            total_tokens: 0,
        };
        let mut budget = BudgetEnforcer::new(self.config.runtime_limits().clone());
        let mut escalation = EscalationDetector::new(self.escalation_config.clone());
        let mut exec = ExecutionContext {
            trace_id: &trace_id,
            options: &options,
            budget: &mut budget,
            escalation: &mut escalation,
        };

        let result: SwarmResult<Response> = async {
            self.apply_injection_policy(&trace_id, &mut state.history)
                .await?;
            for message in &state.history {
                if let Some(content) = message.content() {
                    self.enforce_content_policy(&trace_id, content, "input_message")
                        .await?;
                }
            }

            let mut termination_reason = None;
            if !steps.steps.is_empty() {
                for step in &steps.steps {
                    let response = self.execute_step(&mut state, step, &mut exec).await?;
                    if let Some(reason) = response.termination_reason {
                        termination_reason = Some(reason);
                        break;
                    }
                }
            } else {
                debug_print(
                    options.debug,
                    "No steps defined. Executing default behavior.",
                );
                let response = self.single_execution(&mut state, &mut exec).await?;
                self.persist_iteration_state(&trace_id, &state).await;
                termination_reason = response.termination_reason;
            }

            self.store_messages_if_configured(&trace_id, &state.history)
                .await;
            if let Ok(serialized_history) = serde_json::to_string(&state.history) {
                self.persist_memory_hook(
                    &trace_id,
                    "history:final",
                    &serialized_history,
                    "conversation_completion",
                )
                .await;
            }

            let loop_reason = termination_reason
                .clone()
                .unwrap_or(crate::phase::TerminationReason::TaskComplete);
            self.emit(AgentEvent::LoopEnd {
                trace_id: trace_id.clone(),
                agent_name: state.agent.name().to_string(),
                iterations: state.iterations,
                total_tokens: state.total_tokens as usize,
                termination_reason: loop_reason.clone(),
                timestamp: Utc::now(),
            })
            .await;
            self.complete_session_if_configured(&trace_id, &loop_reason.to_string())
                .await;

            Ok(Response {
                messages: state.history.clone(),
                agent: Some(state.agent.clone()),
                context_variables: state.context_variables.clone(),
                termination_reason,
                tokens_used: state.total_tokens,
            })
        }
        .await;

        match result {
            Ok(response) => Ok(response),
            Err(err) => {
                self.emit(AgentEvent::Error {
                    trace_id: trace_id.clone(),
                    message: err.to_string(),
                    error_type: "run_error".to_string(),
                    timestamp: Utc::now(),
                })
                .await;
                self.store_messages_if_configured(&trace_id, &state.history)
                    .await;
                self.complete_session_if_configured(&trace_id, &format!("error: {}", err))
                    .await;
                Err(err)
            }
        }
    }

    /// Saves a checkpoint if a `CheckpointStore` is configured.
    ///
    /// Failures are non-fatal — they are traced at WARN level but do not abort
    /// the in-memory agent loop.
    async fn save_checkpoint_if_configured(
        &self,
        session_id: &str,
        agent_name: &str,
        messages: &[Message],
        context_variables: &ContextVariables,
        iteration: u32,
        total_tokens: u32,
    ) {
        let Some(store) = &self.checkpoint_store else {
            return;
        };
        let data = CheckpointData::new(
            messages.to_vec(),
            context_variables.clone(),
            agent_name,
            iteration,
            TokenUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens,
            },
        );
        let envelope = CheckpointEnvelope::new(session_id, data);
        if let Err(e) = store.save_checkpoint(&envelope).await {
            tracing::warn!(
                session_id = %session_id,
                iteration  = iteration,
                "checkpoint save failed (non-fatal): {}",
                e
            );
        }
    }

    /// Resume a previously interrupted session from its most recent checkpoint.
    ///
    /// Requires a `CheckpointStore` to be configured on the `Swarm`.
    /// The saved agent name must still exist in the agent registry.
    ///
    /// Returns a structured error for:
    /// - missing checkpoint store
    /// - no checkpoint found for `session_id`
    /// - incompatible checkpoint version
    /// - unknown agent in checkpoint
    /// - no remaining iterations (checkpoint at or past `max_loop_iterations`)
    pub async fn resume_from_checkpoint(
        &self,
        session_id: &str,
        model_override: Option<String>,
        stream: bool,
        debug: bool,
    ) -> SwarmResult<Response> {
        let store = self.checkpoint_store.as_ref().ok_or_else(|| {
            SwarmError::ConfigError(
                "resume_from_checkpoint requires a checkpoint store; \
                 configure one via SwarmBuilder::with_checkpoint_store"
                    .to_string(),
            )
        })?;

        let envelope = store.load_checkpoint(session_id).await?.ok_or_else(|| {
            SwarmError::Other(format!("No checkpoint found for session '{}'", session_id))
        })?;

        // Validate before any state mutation.
        envelope.validate()?;

        let agent = self
            .get_agent_by_name(&envelope.payload.current_agent)
            .map_err(|_| {
                SwarmError::AgentNotFoundError(format!(
                    "checkpoint references agent '{}' which is not registered",
                    envelope.payload.current_agent
                ))
            })?;

        let remaining = self
            .config
            .max_loop_iterations()
            .saturating_sub(envelope.payload.iteration) as usize;

        if remaining == 0 {
            return Err(SwarmError::MaxIterationsError {
                max: self.config.max_loop_iterations() as usize,
                actual: envelope.payload.iteration as usize,
            });
        }

        self.run(
            agent,
            envelope.payload.messages,
            envelope.payload.context_variables,
            model_override,
            stream,
            debug,
            remaining,
        )
        .await
    }

    pub fn get_agent_by_name(&self, name: &str) -> SwarmResult<Agent> {
        self.agent_directory
            .get(&AgentRef::new(name))
            .map(|agent| (*agent).clone())
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
        if self.request_timeout() < MIN_REQUEST_TIMEOUT
            || self.request_timeout() > MAX_REQUEST_TIMEOUT
        {
            return Err(SwarmError::ValidationError(format!(
                "request_timeout must be between {} and {} seconds",
                MIN_REQUEST_TIMEOUT, MAX_REQUEST_TIMEOUT
            )));
        }
        if self.loop_control().default_max_iterations() == 0 {
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
                if !self
                    .functions()
                    .iter()
                    .any(|function| function.name() == *name)
                {
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
