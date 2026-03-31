use crate::agent_comm::MessageId;
use crate::circuit_breaker::CircuitStateSnapshot;
use crate::distribution::AgentAddress;
use crate::escalation::{EscalationAction, EscalationTrigger};
use crate::guardrails::DataClassification;
use crate::phase::{AgentLoopPhase, PhaseResult, TerminationReason};
use crate::team::{AgentTeam, TeamDecision};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// Opaque identifier for a single agent loop execution.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TraceId(String);

impl TraceId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for TraceId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for TraceId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum AgentEvent {
    #[serde(rename = "loop_start")]
    LoopStart {
        trace_id: TraceId,
        agent_name: String,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "phase_start")]
    PhaseStart {
        trace_id: TraceId,
        phase: AgentLoopPhase,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "phase_end")]
    PhaseEnd {
        trace_id: TraceId,
        phase: AgentLoopPhase,
        result: PhaseResult,
        duration_ms: u64,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "tool_call")]
    ToolCall {
        trace_id: TraceId,
        tool_name: String,
        arguments: Value,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        trace_id: TraceId,
        tool_name: String,
        result: Value,
        success: bool,
        duration_ms: u64,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "llm_request")]
    LlmRequest {
        trace_id: TraceId,
        model: String,
        prompt_tokens: usize,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "llm_response")]
    LlmResponse {
        trace_id: TraceId,
        model: String,
        completion_tokens: usize,
        latency_ms: u64,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "guardrail_triggered")]
    GuardrailTriggered {
        trace_id: TraceId,
        guardrail_type: String,
        action: String,
        details: String,
        classification: Option<DataClassification>,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "budget_exceeded")]
    BudgetExceeded {
        trace_id: TraceId,
        limit_type: String,
        details: String,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "circuit_breaker_state_changed")]
    CircuitBreakerStateChanged {
        trace_id: TraceId,
        breaker_name: String,
        state: CircuitStateSnapshot,
        reason: Option<String>,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "escalation_triggered")]
    EscalationTriggered {
        trace_id: TraceId,
        trigger: EscalationTrigger,
        action: EscalationAction,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "memory_persisted")]
    MemoryPersisted {
        trace_id: TraceId,
        key: String,
        source: String,
        classification: DataClassification,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "loop_end")]
    LoopEnd {
        trace_id: TraceId,
        agent_name: String,
        iterations: u32,
        total_tokens: usize,
        termination_reason: TerminationReason,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "error")]
    Error {
        trace_id: TraceId,
        message: String,
        error_type: String,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "message_sent")]
    MessageSent {
        trace_id: TraceId,
        from: AgentAddress,
        to: AgentAddress,
        message_id: MessageId,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "message_received")]
    MessageReceived {
        trace_id: TraceId,
        by: AgentAddress,
        message_id: MessageId,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "reply_timeout")]
    ReplyTimeout {
        trace_id: TraceId,
        from: AgentAddress,
        to: AgentAddress,
        correlation_id: MessageId,
        timeout_ms: u64,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "team_formed")]
    TeamFormed {
        trace_id: TraceId,
        team: AgentTeam,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "consensus_reached")]
    ConsensusReached {
        trace_id: TraceId,
        decision: TeamDecision,
        timestamp: DateTime<Utc>,
    },
}

impl AgentEvent {
    pub fn trace_id(&self) -> &str {
        match self {
            Self::LoopStart { trace_id, .. } => trace_id.as_str(),
            Self::PhaseStart { trace_id, .. } => trace_id.as_str(),
            Self::PhaseEnd { trace_id, .. } => trace_id.as_str(),
            Self::ToolCall { trace_id, .. } => trace_id.as_str(),
            Self::ToolResult { trace_id, .. } => trace_id.as_str(),
            Self::LlmRequest { trace_id, .. } => trace_id.as_str(),
            Self::LlmResponse { trace_id, .. } => trace_id.as_str(),
            Self::GuardrailTriggered { trace_id, .. } => trace_id.as_str(),
            Self::BudgetExceeded { trace_id, .. } => trace_id.as_str(),
            Self::CircuitBreakerStateChanged { trace_id, .. } => trace_id.as_str(),
            Self::EscalationTriggered { trace_id, .. } => trace_id.as_str(),
            Self::MemoryPersisted { trace_id, .. } => trace_id.as_str(),
            Self::LoopEnd { trace_id, .. } => trace_id.as_str(),
            Self::Error { trace_id, .. } => trace_id.as_str(),
            Self::MessageSent { trace_id, .. } => trace_id.as_str(),
            Self::MessageReceived { trace_id, .. } => trace_id.as_str(),
            Self::ReplyTimeout { trace_id, .. } => trace_id.as_str(),
            Self::TeamFormed { trace_id, .. } => trace_id.as_str(),
            Self::ConsensusReached { trace_id, .. } => trace_id.as_str(),
        }
    }

    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            Self::LoopStart { timestamp, .. } => *timestamp,
            Self::PhaseStart { timestamp, .. } => *timestamp,
            Self::PhaseEnd { timestamp, .. } => *timestamp,
            Self::ToolCall { timestamp, .. } => *timestamp,
            Self::ToolResult { timestamp, .. } => *timestamp,
            Self::LlmRequest { timestamp, .. } => *timestamp,
            Self::LlmResponse { timestamp, .. } => *timestamp,
            Self::GuardrailTriggered { timestamp, .. } => *timestamp,
            Self::BudgetExceeded { timestamp, .. } => *timestamp,
            Self::CircuitBreakerStateChanged { timestamp, .. } => *timestamp,
            Self::EscalationTriggered { timestamp, .. } => *timestamp,
            Self::MemoryPersisted { timestamp, .. } => *timestamp,
            Self::LoopEnd { timestamp, .. } => *timestamp,
            Self::Error { timestamp, .. } => *timestamp,
            Self::MessageSent { timestamp, .. } => *timestamp,
            Self::MessageReceived { timestamp, .. } => *timestamp,
            Self::ReplyTimeout { timestamp, .. } => *timestamp,
            Self::TeamFormed { timestamp, .. } => *timestamp,
            Self::ConsensusReached { timestamp, .. } => *timestamp,
        }
    }
}

impl fmt::Display for AgentEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LoopStart { agent_name, .. } => write!(f, "LoopStart({})", agent_name),
            Self::PhaseStart { phase, .. } => write!(f, "PhaseStart({})", phase),
            Self::PhaseEnd {
                phase, duration_ms, ..
            } => write!(f, "PhaseEnd({}, {}ms)", phase, duration_ms),
            Self::ToolCall { tool_name, .. } => write!(f, "ToolCall({})", tool_name),
            Self::ToolResult {
                tool_name, success, ..
            } => {
                write!(
                    f,
                    "ToolResult({}, {})",
                    tool_name,
                    if *success { "ok" } else { "err" }
                )
            }
            Self::LlmRequest { model, .. } => write!(f, "LlmRequest({})", model),
            Self::LlmResponse {
                model, latency_ms, ..
            } => write!(f, "LlmResponse({}, {}ms)", model, latency_ms),
            Self::GuardrailTriggered {
                guardrail_type,
                action,
                ..
            } => write!(f, "GuardrailTriggered({}, {})", guardrail_type, action),
            Self::BudgetExceeded { limit_type, .. } => write!(f, "BudgetExceeded({})", limit_type),
            Self::CircuitBreakerStateChanged {
                breaker_name,
                state,
                ..
            } => write!(f, "CircuitBreakerStateChanged({}, {})", breaker_name, state),
            Self::EscalationTriggered {
                trigger, action, ..
            } => {
                write!(f, "EscalationTriggered({}, {:?})", trigger, action)
            }
            Self::MemoryPersisted { key, source, .. } => {
                write!(f, "MemoryPersisted({}, {})", source, key)
            }
            Self::LoopEnd {
                agent_name,
                iterations,
                ..
            } => write!(f, "LoopEnd({}, {} iters)", agent_name, iterations),
            Self::Error { message, .. } => {
                write!(f, "Error({})", crate::util::safe_truncate(message, 200))
            }
            Self::MessageSent { from, to, .. } => write!(f, "MessageSent({} → {})", from, to),
            Self::MessageReceived { by, .. } => write!(f, "MessageReceived({})", by),
            Self::ReplyTimeout { from, to, .. } => write!(f, "ReplyTimeout({} → {})", from, to),
            Self::TeamFormed { team, .. } => {
                write!(f, "TeamFormed(assignments={})", team.assignments().len())
            }
            Self::ConsensusReached { decision, .. } => {
                write!(f, "ConsensusReached({})", decision.selected_option())
            }
        }
    }
}

#[async_trait]
pub trait EventSubscriber: Send + Sync {
    async fn on_event(&self, event: &AgentEvent);
}

pub struct LoggingSubscriber;

impl LoggingSubscriber {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LoggingSubscriber {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventSubscriber for LoggingSubscriber {
    async fn on_event(&self, event: &AgentEvent) {
        tracing::info!(
            trace_id = %event.trace_id(),
            "[{}] {}",
            event.timestamp().format("%Y-%m-%d %H:%M:%S"),
            event
        );
    }
}
