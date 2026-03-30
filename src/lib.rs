pub mod constants;
pub mod core;
pub mod types;
pub mod util;
pub mod validation;
pub mod stream;

pub mod error;
pub mod phase;
pub mod event;
pub mod tool;
pub mod memory;
pub mod provider;
pub mod guardrails;

#[cfg(test)]
pub mod tests;
pub use crate::core::Swarm;
pub use crate::types::{
    Agent, ContextVariables, FunctionCall, FunctionCallPolicy, Instructions, Message, MessageRole,
    Response, SwarmConfig, ToolCallExecution,
};
pub use crate::error::{SwarmError, SwarmResult};
pub use crate::phase::{AgentLoop, AgentLoopPhase, PlannedAction, TerminationReason, PhaseResult, TokenUsage};
pub use crate::event::{AgentEvent, EventSubscriber, LoggingSubscriber};
pub use crate::tool::{
    ClosureTool, InvocationArgs, Tool, ToolError, ToolRegistry, ToolSchema, ToolCallSpec, ToolResult,
};
pub use crate::memory::{Memory, SlidingWindowMemory};
pub use crate::provider::{LlmProvider, CompletionRequest, CompletionResponse, Chunk, OpenAiProvider};
pub use crate::guardrails::{
    detect_prompt_injection, detect_prompt_injection_with_sanitization,
    InjectionCheckResult, redact_pii, redact_pii_with, contains_pii, find_pii,
};
