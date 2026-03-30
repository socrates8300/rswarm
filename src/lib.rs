pub mod constants;
pub mod core;
pub mod stream;
pub mod types;
pub mod util;
pub mod validation;

pub mod checkpoint;
pub mod circuit_breaker;
pub mod error;
pub mod escalation;
pub mod event;
pub mod guardrails;
pub mod memory;
pub mod observability;
pub mod persistence;
pub mod phase;
pub mod provider;
pub mod tool;

#[cfg(test)]
pub mod tests;
pub use crate::checkpoint::{CheckpointData, CheckpointEnvelope, CURRENT_CHECKPOINT_VERSION};
pub use crate::circuit_breaker::{CircuitBreaker, CircuitStateSnapshot};
pub use crate::core::Swarm;
pub use crate::error::{SwarmError, SwarmResult};
pub use crate::escalation::{
    EscalationAction, EscalationConfig, EscalationDetector, EscalationTrigger,
};
pub use crate::event::{AgentEvent, EventSubscriber, LoggingSubscriber, TraceId};
pub use crate::guardrails::{
    apply_redaction_policy, check_injection_with_policy, classify_and_redact, classify_text,
    contains_pii, detect_prompt_injection, detect_prompt_injection_with_sanitization, find_pii,
    redact_pii, redact_pii_with, ContentPolicy, DataClassification, DefaultContentPolicy,
    InjectionCheckResult, InjectionOutcome, InjectionPolicy, PolicyResult, RedactionPolicy,
};
pub use crate::memory::vector::{InMemoryVectorStore, MemoryEntry, RetrievalPolicy, VectorMemory};
pub use crate::memory::{Memory, SlidingWindowMemory};
#[cfg(feature = "postgres")]
pub use crate::persistence::postgres::PostgresStore;
pub use crate::persistence::sqlite::SqliteStore;
pub use crate::persistence::{
    CheckpointStore, CheckpointSummary, EventStore, MemoryRecord, MemoryStore, PersistenceBackend,
    SessionRecord, SessionStore,
};
pub use crate::phase::{
    AgentLoop, AgentLoopPhase, PhaseResult, PlannedAction, TerminationReason, TokenUsage,
};
pub use crate::provider::{
    Chunk, CompletionRequest, CompletionResponse, LlmProvider, OpenAiProvider,
};
pub use crate::tool::{
    ClosureTool, InvocationArgs, Tool, ToolCallSpec, ToolError, ToolRegistry, ToolResult,
    ToolSchema,
};
pub use crate::types::RuntimeLimits;
pub use crate::types::{
    Agent, ContextVariables, FunctionCall, FunctionCallPolicy, Instructions, Message, MessageRole,
    Response, SwarmConfig, ToolCallExecution,
};
pub use crate::validation::{
    verify_structured_response, verify_tool_arguments, BudgetEnforcer, BudgetExhausted,
};
