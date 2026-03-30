use crate::error::SwarmResult;
use crate::tool::{ToolCallSpec, ToolResult, ToolSchema};
use crate::types::{ContextVariables, Message};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLoopPhase {
    Perceive {
        messages: Vec<Message>,
        context_variables: ContextVariables,
        available_tools: Vec<ToolSchema>,
    },
    Reason {
        prompt: String,
        thoughts: Option<String>,
    },
    Plan {
        planned_actions: Vec<PlannedAction>,
        requires_approval: bool,
    },
    Act {
        tool_calls: Vec<ToolCallSpec>,
        parallel: bool,
    },
    Observe {
        results: Vec<ToolResult>,
        should_continue: bool,
        termination_reason: Option<TerminationReason>,
    },
}

impl fmt::Display for AgentLoopPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Perceive { .. } => write!(f, "Perceive"),
            Self::Reason { .. } => write!(f, "Reason"),
            Self::Plan { .. } => write!(f, "Plan"),
            Self::Act { .. } => write!(f, "Act"),
            Self::Observe { .. } => write!(f, "Observe"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlannedAction {
    pub tool: String,
    pub args: Value,
    pub rationale: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TerminationReason {
    TaskComplete,
    MaxIterations,
    TokenBudgetExhausted,
    Error(String),
    ExplicitStop,
    DoomLoopDetected,
}

impl fmt::Display for TerminationReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TaskComplete => write!(f, "Task completed"),
            Self::MaxIterations => write!(f, "Max iterations reached"),
            Self::TokenBudgetExhausted => write!(f, "Token budget exhausted"),
            Self::Error(msg) => write!(f, "Error: {}", msg),
            Self::ExplicitStop => write!(f, "Explicit stop"),
            Self::DoomLoopDetected => write!(f, "Doom loop detected"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseResult {
    Success {
        phase: AgentLoopPhase,
        output: Value,
        duration_ms: u64,
        tokens_used: Option<TokenUsage>,
        termination_reason: Option<TerminationReason>,
    },
    Failure {
        phase: AgentLoopPhase,
        error: String,
        retryable: bool,
        duration_ms: u64,
        termination_reason: Option<TerminationReason>,
    },
    Skipped {
        phase: AgentLoopPhase,
        reason: String,
        termination_reason: Option<TerminationReason>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl PhaseResult {
    pub fn success(phase: AgentLoopPhase, output: Value, duration_ms: u64) -> Self {
        Self::Success {
            phase,
            output,
            duration_ms,
            tokens_used: None,
            termination_reason: None,
        }
    }

    pub fn failure(
        phase: AgentLoopPhase,
        error: String,
        retryable: bool,
        duration_ms: u64,
    ) -> Self {
        Self::Failure {
            phase,
            error,
            retryable,
            duration_ms,
            termination_reason: None,
        }
    }

    pub fn skipped(phase: AgentLoopPhase, reason: impl Into<String>) -> Self {
        Self::Skipped {
            phase,
            reason: reason.into(),
            termination_reason: None,
        }
    }

    pub fn with_tokens(mut self, tokens: TokenUsage) -> Self {
        if let Self::Success { tokens_used, .. } = &mut self {
            *tokens_used = Some(tokens);
        }
        self
    }

    pub fn with_termination_reason(mut self, termination_reason: TerminationReason) -> Self {
        match &mut self {
            Self::Success {
                termination_reason: slot,
                ..
            }
            | Self::Failure {
                termination_reason: slot,
                ..
            }
            | Self::Skipped {
                termination_reason: slot,
                ..
            } => *slot = Some(termination_reason),
        }
        self
    }

    pub fn duration_ms(&self) -> u64 {
        match self {
            Self::Success { duration_ms, .. } | Self::Failure { duration_ms, .. } => *duration_ms,
            Self::Skipped { .. } => 0,
        }
    }
}

/// Hook trait for structured ReAct loop execution.
///
/// Implementors can intercept each phase of the agent loop for custom
/// behaviour, tracing, or policy enforcement.
#[async_trait]
pub trait AgentLoop: Send + Sync {
    async fn execute_phase(&mut self, phase: AgentLoopPhase) -> SwarmResult<PhaseResult>;
    fn on_phase_start(&mut self, _phase: &AgentLoopPhase) {}
    fn on_phase_end(&mut self, _phase: &AgentLoopPhase, _result: &PhaseResult) {}
}

impl fmt::Display for PhaseResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Success { phase, .. } => {
                write!(f, "PhaseResult::Success {{ phase: {}, duration: {}ms }}", phase, self.duration_ms())
            }
            Self::Failure { phase, error, .. } => write!(
                f,
                "PhaseResult::Failure {{ phase: {}, duration: {}ms, error: {} }}",
                phase,
                self.duration_ms(),
                error
            ),
            Self::Skipped { phase, reason, .. } => {
                write!(f, "PhaseResult::Skipped {{ phase: {}, reason: {} }}", phase, reason)
            }
        }
    }
}
