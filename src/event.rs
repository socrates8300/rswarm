use crate::phase::{AgentLoopPhase, PhaseResult, TerminationReason};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum AgentEvent {
    #[serde(rename = "loop_start")]
    LoopStart {
        trace_id: String,
        agent_name: String,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "phase_start")]
    PhaseStart {
        trace_id: String,
        phase: AgentLoopPhase,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "phase_end")]
    PhaseEnd {
        trace_id: String,
        phase: AgentLoopPhase,
        result: PhaseResult,
        duration_ms: u64,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "tool_call")]
    ToolCall {
        trace_id: String,
        tool_name: String,
        arguments: Value,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        trace_id: String,
        tool_name: String,
        result: Value,
        success: bool,
        duration_ms: u64,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "llm_request")]
    LlmRequest {
        trace_id: String,
        model: String,
        prompt_tokens: usize,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "llm_response")]
    LlmResponse {
        trace_id: String,
        model: String,
        completion_tokens: usize,
        latency_ms: u64,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "loop_end")]
    LoopEnd {
        trace_id: String,
        agent_name: String,
        iterations: u32,
        total_tokens: usize,
        termination_reason: TerminationReason,
        timestamp: DateTime<Utc>,
    },
    #[serde(rename = "error")]
    Error {
        trace_id: String,
        message: String,
        error_type: String,
        timestamp: DateTime<Utc>,
    },
}

impl AgentEvent {
    pub fn trace_id(&self) -> &str {
        match self {
            Self::LoopStart { trace_id, .. } => trace_id,
            Self::PhaseStart { trace_id, .. } => trace_id,
            Self::PhaseEnd { trace_id, .. } => trace_id,
            Self::ToolCall { trace_id, .. } => trace_id,
            Self::ToolResult { trace_id, .. } => trace_id,
            Self::LlmRequest { trace_id, .. } => trace_id,
            Self::LlmResponse { trace_id, .. } => trace_id,
            Self::LoopEnd { trace_id, .. } => trace_id,
            Self::Error { trace_id, .. } => trace_id,
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
            Self::LoopEnd { timestamp, .. } => *timestamp,
            Self::Error { timestamp, .. } => *timestamp,
        }
    }
}

impl fmt::Display for AgentEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LoopStart { agent_name, .. } => write!(f, "LoopStart({})", agent_name),
            Self::PhaseStart { phase, .. } => write!(f, "PhaseStart({})", phase),
            Self::PhaseEnd { phase, duration_ms, .. } => write!(f, "PhaseEnd({}, {}ms)", phase, duration_ms),
            Self::ToolCall { tool_name, .. } => write!(f, "ToolCall({})", tool_name),
            Self::ToolResult { tool_name, success, .. } => {
                write!(f, "ToolResult({}, {})", tool_name, if *success { "ok" } else { "err" })
            }
            Self::LlmRequest { model, .. } => write!(f, "LlmRequest({})", model),
            Self::LlmResponse { model, latency_ms, .. } => write!(f, "LlmResponse({}, {}ms)", model, latency_ms),
            Self::LoopEnd { agent_name, iterations, .. } => write!(f, "LoopEnd({}, {} iters)", agent_name, iterations),
            Self::Error { message, .. } => write!(f, "Error({})", message),
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
        println!("[{}] {}", event.timestamp().format("%Y-%m-%d %H:%M:%S"), event);
    }
}
