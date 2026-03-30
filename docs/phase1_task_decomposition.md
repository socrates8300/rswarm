# Phase 1: Core Loop and Single Agent — Task Decomposition

> **Source**: PRD Section 11.1 — Minimal Viable Harness with ReAct Loop  
> **Created**: 2025-03-29  
> **Database**: `docs/agent_context.db`

---

## 1. Overview

### 1.1 Phase 1 Goals (from PRD)

Deliver a minimal viable harness implementing:
- **Core ReAct loop** with explicit perception-reasoning-action-observation phases
- **Trait-based tool definition** with derive macros for schema generation
- **In-memory state management** with sliding window context
- **OpenAI-compatible provider abstraction** with streaming support
- **OpenTelemetry integration** for basic tracing and metrics

### 1.2 Success Criteria (PRD 11.1.1)

- [ ] Developer can create functional agent in **<50 lines of Rust**
- [ ] All core behaviors are **traceable and observable**
- [ ] Test suite covers **90%+ of code paths**
- [ ] No type safety escapes (`as Any`, excessive `unwrap`)

---

## 2. Current State Assessment

### 2.1 What Exists

| Component | Location | Status |
|-----------|----------|--------|
| Swarm orchestrator | `src/core.rs` | ✅ Functional |
| SwarmBuilder pattern | `src/core.rs` | ✅ Functional |
| AgentFunction (closure-based) | `src/types.rs` | ✅ Functional |
| ContextVariables | `src/types.rs` | ✅ Functional |
| OpenAI HTTP client | `src/core.rs` | ✅ Hardcoded to OpenAI |
| Streaming support | `src/stream.rs` | ✅ Functional |
| XML steps parsing | `src/util.rs` | ✅ Functional |
| Error types (SwarmError) | `src/error.rs` | ✅ Comprehensive |
| Unit/integration tests | `src/tests/` | ✅ Good coverage |

### 2.2 What's Missing (Gaps to Address)

| Gap | PRD Reference | Priority |
|-----|---------------|----------|
| Explicit ReAct phases | Section 2.1 | Critical |
| Tool trait system | Section 4.1 | Critical |
| Token budget management | Section 3.1.2 | High |
| LLM provider abstraction | Section 5.1 | Critical |
| OpenTelemetry integration | Section 6.3 | Medium |
| State checkpointing | Section 3.3 | Medium |
| Safety guardrails | Section 7.1-7.3 | Medium |

---

## 3. Task Breakdown

### 3.1 Core Loop (ReAct Pattern)

| ID | Title | Priority | Dependencies | Location |
|----|-------|----------|--------------|----------|
| #1 | Define AgentLoopPhase enum for explicit ReAct phases | critical | none | `src/types.rs` |
| #2 | Implement AgentLoop trait with execute_phase method | critical | #1 | `src/core.rs` |
| #3 | Refactor Swarm::run to use explicit phase transitions | high | #1, #2 | `src/core.rs` |
| #4 | Add iteration counter and phase timing metrics | medium | #3 | `src/types.rs` |

#### Task #1: Define AgentLoopPhase enum

**Depends on**: none  
**Acceptance criteria**:
- Enum with variants: `Perceive`, `Reason`, `Plan`, `Act`, `Observe`
- Each variant has associated data structures
- Implements `Display` for logging

```rust
// Target API
pub enum AgentLoopPhase {
    Perceive { context: Vec<Message> },
    Reason { prompt: String },
    Plan { steps: Vec<PlannedAction> },
    Act { tool_calls: Vec<ToolCall> },
    Observe { results: Vec<ToolResult> },
}
```

**Estimate**: small (30 min)

---

#### Task #2: Implement AgentLoop trait

**Depends on**: #1  
**Acceptance criteria**:
- Trait defines `async fn execute_phase(&mut self, phase: AgentLoopPhase) -> SwarmResult<PhaseResult>`
- Default implementation for phase transitions
- Supports hooks for before/after each phase

```rust
// Target API
#[async_trait]
pub trait AgentLoop: Send + Sync {
    async fn execute_phase(&mut self, phase: AgentLoopPhase) -> SwarmResult<PhaseResult>;
    
    fn on_phase_start(&mut self, phase: &AgentLoopPhase) { }
    fn on_phase_end(&mut self, phase: &AgentLoopPhase, result: &PhaseResult) { }
}
```

**Estimate**: medium (45 min)

---

#### Task #3: Refactor Swarm::run with phases

**Depends on**: #1, #2  
**Acceptance criteria**:
- `run()` iterates through Perceive→Reason→Act→Observe cycle
- Each phase emits trace events
- Backward compatible with existing XML steps

**Current code** (`src/core.rs` lines 609-684):
```rust
// Current: single_execution loop
// Target: explicit phase transitions
```

**Estimate**: medium (60 min)

---

#### Task #4: Add iteration counter and timing

**Depends on**: #3  
**Acceptance criteria**:
- Track iteration count per `run()`
- Record phase execution duration
- Expose via `Response` struct

**Estimate**: small (20 min)

---

### 3.2 Tool System (Trait-Based)

| ID | Title | Priority | Dependencies | Location |
|----|-------|----------|--------------|----------|
| #5 | Define Tool trait with name, description, schema, execute | critical | none | `src/tool.rs` (new) |
| #6 | Create ToolError type with error classification | high | #5 | `src/error.rs` |
| #7 | Implement ToolRegistry with HashMap storage | high | #5 | `src/tool.rs` |
| #8 | Create Tool derive macro for schema generation | medium | #5, #6 | `src/derive.rs` (new) |
| #9 | Migrate AgentFunction to use Tool trait internally | high | #5, #7 | `src/types.rs` |

#### Task #5: Define Tool trait

**Depends on**: none  
**Acceptance criteria**:
- `pub trait Tool: Send + Sync + 'static`
- `fn name(&self) -> &str`
- `fn description(&self) -> &str`
- `fn parameters_schema(&self) -> Value`
- `async fn execute(&self, args: Value) -> Result<Value, ToolError>`

```rust
// Target API (PRD Section 4.1.1)
#[async_trait]
pub trait Tool: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn parameters_schema(&self) -> Value;
    async fn execute(&self, args: Value) -> Result<Value, ToolError>;
}
```

**Estimate**: small (30 min)

---

#### Task #6: Create ToolError type

**Depends on**: #5  
**Acceptance criteria**:
- `ToolError` enum with variants: `Validation`, `Execution`, `Timeout`, `Network`
- `is_retryable()` method
- `From` impls for common error types

```rust
#[derive(Error, Debug)]
pub enum ToolError {
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Execution error: {0}")]
    Execution(String),
    #[error("Timeout after {0}ms")]
    Timeout(u64),
    #[error("Network error: {0}")]
    Network(String),
}
```

**Estimate**: small (20 min)

---

#### Task #7: Implement ToolRegistry

**Depends on**: #5  
**Acceptance criteria**:
- `register(tool: Arc<dyn Tool>)` method
- `get(name: &str) -> Option<Arc<dyn Tool>>`
- `list_all() -> Vec<Arc<dyn Tool>>`
- `to_openai_functions() -> Vec<Value>` for API serialization

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn register(&mut self, tool: Arc<dyn Tool>) { }
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> { }
    pub fn to_openai_functions(&self) -> Vec<Value> { }
}
```

**Estimate**: small (30 min)

---

#### Task #8: Create Tool derive macro

**Depends on**: #5, #6  
**Acceptance criteria**:
- `#[derive(Tool)]` generates `Tool` impl
- `#[tool(name = "...", description = "...")]` attributes
- `#[param(description = "...")]` for fields
- Uses `schemars` for JSON Schema generation

```rust
// Target usage
#[derive(Tool)]
#[tool(name = "read_file", description = "Read file contents")]
struct ReadFile {
    #[param(description = "File path to read")]
    path: String,
}
```

**Estimate**: medium (60 min)

---

#### Task #9: Migrate AgentFunction to Tool trait

**Depends on**: #5, #7  
**Acceptance criteria**:
- `Agent.functions` becomes `Vec<Arc<dyn Tool>>`
- Backward compatible API for closure-based tools
- All existing tests pass

**Estimate**: medium (45 min)

---

### 3.3 Memory and State Management

| ID | Title | Priority | Dependencies | Location |
|----|-------|----------|--------------|----------|
| #10 | Define Memory trait with store, retrieve, clear methods | critical | none | `src/memory.rs` (new) |
| #11 | Implement SlidingWindowMemory with configurable size | high | #10 | `src/memory.rs` |
| #12 | Add TokenCounter utility with tiktoken-style estimation | high | none | `src/util.rs` |
| #13 | Implement context pruning at token budget thresholds | medium | #11, #12 | `src/core.rs` |
| #14 | Define Checkpoint trait for state serialization | medium | none | `src/checkpoint.rs` (new) |

#### Task #10: Define Memory trait

**Depends on**: none  
**Acceptance criteria**:
- `pub trait Memory: Send + Sync`
- `async fn store(&mut self, key: &str, value: &str) -> SwarmResult<()>`
- `async fn retrieve(&self, key: &str) -> SwarmResult<Option<String>>`
- `async fn clear(&mut self) -> SwarmResult<()>`

```rust
#[async_trait]
pub trait Memory: Send + Sync {
    async fn store(&mut self, key: &str, value: &str) -> SwarmResult<()>;
    async fn retrieve(&self, key: &str) -> SwarmResult<Option<String>>;
    async fn clear(&mut self) -> SwarmResult<()>;
}
```

**Estimate**: small (20 min)

---

#### Task #11: Implement SlidingWindowMemory

**Depends on**: #10  
**Acceptance criteria**:
- Configurable `max_messages` parameter
- FIFO eviction when limit exceeded
- Tracks token count approximately

```rust
pub struct SlidingWindowMemory {
    messages: VecDeque<Message>,
    max_messages: usize,
    token_counter: TokenCounter,
}
```

**Estimate**: small (30 min)

---

#### Task #12: Add TokenCounter utility

**Depends on**: none  
**Acceptance criteria**:
- `count_tokens(text: &str) -> usize` function
- Reasonable accuracy for common models (GPT-4, Claude)
- Zero external dependencies or optional feature

```rust
pub struct TokenCounter {
    model: TokenizerModel,
}

impl TokenCounter {
    pub fn count_tokens(&self, text: &str) -> usize { }
}
```

**Estimate**: medium (45 min)

---

#### Task #13: Implement context pruning

**Depends on**: #11, #12  
**Acceptance criteria**:
- Prune at 80% (mask old observations)
- Prune at 85% (remove old tool outputs)
- Log warnings at 70% capacity

```rust
// In get_chat_completion
if token_ratio > 0.70 { warn!("Context at 70% capacity"); }
if token_ratio > 0.80 { mask_old_observations(); }
if token_ratio > 0.85 { remove_old_tool_outputs(); }
```

**Estimate**: medium (45 min)

---

#### Task #14: Define Checkpoint trait

**Depends on**: none  
**Acceptance criteria**:
- `trait Checkpoint: Serialize + Deserialize`
- `CheckpointData` struct with messages, context, iteration
- `to_json()` and `from_json()` methods

```rust
#[derive(Serialize, Deserialize)]
pub struct CheckpointData {
    pub messages: Vec<Message>,
    pub context_variables: ContextVariables,
    pub iteration: u32,
    pub agent_name: String,
    pub created_at: DateTime<Utc>,
}
```

**Estimate**: small (30 min)

---

### 3.4 LLM Provider Abstraction

| ID | Title | Priority | Dependencies | Location |
|----|-------|----------|--------------|----------|
| #15 | Define LlmProvider trait with complete and stream methods | critical | none | `src/provider.rs` (new) |
| #16 | Create CompletionRequest and CompletionResponse types | high | none | `src/provider.rs` |
| #17 | Implement OpenAI provider client | high | #15, #16 | `src/provider/openai.rs` (new) |
| #18 | Add Anthropic-compatible provider adapter | medium | #15, #16 | `src/provider/anthropic.rs` (new) |
| #19 | Create ProviderRegistry for multi-provider support | medium | #15, #17 | `src/provider.rs` |

#### Task #15: Define LlmProvider trait

**Depends on**: none  
**Acceptance criteria**:
- `pub trait LlmProvider: Send + Sync`
- `async fn complete(&self, request: CompletionRequest) -> SwarmResult<CompletionResponse>`
- `async fn stream(&self, request: CompletionRequest) -> SwarmResult<impl Stream<Item=Chunk>>`
- `fn model_name(&self) -> &str`

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, request: CompletionRequest) -> SwarmResult<CompletionResponse>;
    async fn stream(&self, request: CompletionRequest) -> SwarmResult<Box<dyn Stream<Item = Chunk>>>;
    fn model_name(&self) -> &str;
}
```

**Estimate**: small (30 min)

---

#### Task #16: Create CompletionRequest/Response types

**Depends on**: none  
**Acceptance criteria**:
- `CompletionRequest`: messages, model, tools, stream flag, temperature
- `CompletionResponse`: id, choices, usage, model
- Serde derives for serialization

```rust
pub struct CompletionRequest {
    pub messages: Vec<Message>,
    pub model: String,
    pub tools: Vec<ToolDefinition>,
    pub stream: bool,
    pub temperature: Option<f32>,
}
```

**Estimate**: small (30 min)

---

#### Task #17: Implement OpenAI provider

**Depends on**: #15, #16  
**Acceptance criteria**:
- Implements `LlmProvider` trait
- Supports streaming via SSE
- Handles rate limits with retry
- All existing tests pass with new provider

**Estimate**: medium (60 min)

---

#### Task #18: Add Anthropic adapter

**Depends on**: #15, #16  
**Acceptance criteria**:
- Implements `LlmProvider` trait
- Converts message format to Anthropic schema
- Handles extended thinking mode

**Estimate**: medium (45 min)

---

#### Task #19: Create ProviderRegistry

**Depends on**: #15, #17  
**Acceptance criteria**:
- Register providers by name
- Select provider by model prefix
- Fallback chain configuration

```rust
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn LlmProvider>>,
    default: String,
}
```

**Estimate**: small (30 min)

---

### 3.5 Observability

| ID | Title | Priority | Dependencies | Location |
|----|-------|----------|--------------|----------|
| #20 | Define AgentEvent enum for structured tracing | high | none | `src/event.rs` (new) |
| #21 | Implement EventSubscriber trait for observers | medium | #20 | `src/event.rs` |
| #22 | Add OpenTelemetry integration with tracing crate | medium | #20, #21 | `Cargo.toml`, `src/observability.rs` (new) |
| #23 | Create JsonlFileSubscriber for session persistence | medium | #20, #21 | `src/event.rs` |

#### Task #20: Define AgentEvent enum

**Depends on**: none  
**Acceptance criteria**:
- Variants: `LoopStart`, `PhaseStart`, `PhaseEnd`, `ToolCall`, `ToolResult`, `LlmRequest`, `LlmResponse`, `LoopEnd`
- Each variant carries relevant data (timing, tokens, etc.)
- Implements `Serialize` for export

```rust
#[derive(Serialize, Clone, Debug)]
pub enum AgentEvent {
    LoopStart { trace_id: String, agent: String },
    PhaseStart { trace_id: String, phase: AgentLoopPhase, timestamp: DateTime<Utc> },
    PhaseEnd { trace_id: String, phase: AgentLoopPhase, duration_ms: u64, result: PhaseResult },
    ToolCall { trace_id: String, tool: String, args: Value },
    ToolResult { trace_id: String, tool: String, result: Value, duration_ms: u64 },
    LlmRequest { trace_id: String, model: String, prompt_tokens: usize },
    LlmResponse { trace_id: String, completion_tokens: usize, latency_ms: u64 },
    LoopEnd { trace_id: String, iterations: u32, total_tokens: usize },
}
```

**Estimate**: small (30 min)

---

#### Task #21: Implement EventSubscriber trait

**Depends on**: #20  
**Acceptance criteria**:
- `trait EventSubscriber: Send + Sync`
- `async fn on_event(&self, event: AgentEvent)`
- Support multiple subscribers via `Vec`

```rust
#[async_trait]
pub trait EventSubscriber: Send + Sync {
    async fn on_event(&self, event: &AgentEvent);
}
```

**Estimate**: small (20 min)

---

#### Task #22: Add OpenTelemetry integration

**Depends on**: #20, #21  
**Acceptance criteria**:
- `otel` feature flag in `Cargo.toml`
- Spans for each phase and tool call
- Metrics for token usage, latency, errors
- Export to OTLP endpoint

**Cargo.toml additions**:
```toml
[dependencies]
opentelemetry = { version = "0.22", optional = true }
opentelemetry-otlp = { version = "0.15", optional = true }
tracing-opentelemetry = { version = "0.23", optional = true }

[features]
otel = ["opentelemetry", "opentelemetry-otlp", "tracing-opentelemetry"]
```

**Estimate**: medium (60 min)

---

#### Task #23: Create JsonlFileSubscriber

**Depends on**: #20, #21  
**Acceptance criteria**:
- Appends events to `.jsonl` file
- Configurable file path
- Rotation by size or time

```rust
pub struct JsonlFileSubscriber {
    path: PathBuf,
    file: File,
}

impl EventSubscriber for JsonlFileSubscriber {
    async fn on_event(&self, event: &AgentEvent) {
        // Append serialized event to file
    }
}
```

**Estimate**: small (30 min)

---

### 3.6 Safety and Guardrails

| ID | Title | Priority | Dependencies | Location |
|----|-------|----------|--------------|----------|
| #24 | Add MaxIterationsError with graceful termination | high | #3 | `src/error.rs` |
| #25 | Implement doom-loop detection for repeated tool calls | medium | #3 | `src/core.rs` |
| #26 | Add prompt injection pattern detection | medium | none | `src/guardrails.rs` (new) |
| #27 | Implement PII redaction for logs | low | none | `src/guardrails.rs` |

#### Task #24: Add MaxIterationsError

**Depends on**: #3  
**Acceptance criteria**:
- Error includes iteration count and last message
- `Swarm::run` returns partial `Response` on limit
- Configurable via `SwarmConfig.max_loop_iterations`

```rust
// In SwarmError enum (already exists, enhance it)
#[error("Maximum iterations ({0}) exceeded")]
MaxIterationsError {
    max: u32,
    actual: u32,
    last_message: Option<String>,
}
```

**Estimate**: small (20 min)

---

#### Task #25: Implement doom-loop detection

**Depends on**: #3  
**Acceptance criteria**:
- Track last N tool calls with args
- Detect identical calls within window
- Inject warning into context or terminate

```rust
pub struct DoomLoopDetector {
    window_size: usize,
    recent_calls: VecDeque<(String, Value)>,
}

impl DoomLoopDetector {
    pub fn check(&mut self, tool: &str, args: &Value) -> Option<DoomLoopWarning>;
}
```

**Estimate**: small (30 min)

---

#### Task #26: Add prompt injection detection

**Depends on**: none  
**Acceptance criteria**:
- Regex patterns for common injection attempts
- Configurable action: `warn`, `sanitize`, `reject`
- Log detected attempts

```rust
pub struct InjectionDetector {
    patterns: Vec<Regex>,
    action: InjectionAction,
}

pub enum InjectionAction {
    Warn,
    Sanitize,
    Reject,
}
```

**Estimate**: small (30 min)

---

#### Task #27: Implement PII redaction

**Depends on**: none  
**Acceptance criteria**:
- Regex for email, phone, credit card patterns
- Applied to event payloads before logging
- Configurable enable/disable

```rust
pub fn redact_pii(text: &str) -> String {
    // Replace emails, phones, CC numbers with [REDACTED]
}
```

**Estimate**: small (20 min)

---

## 4. Execution Order

### Phase 1A — Foundation Traits (Parallel-Safe)

```
┌─────────────────────────────────────────────────────────────┐
│  MAX PARALLELISM: 8 tasks                                   │
│  No interdependencies — can all start simultaneously        │
└─────────────────────────────────────────────────────────────┘

  #1  Define AgentLoopPhase enum              [critical]  30m
  #5  Define Tool trait                       [critical]  30m
  #10 Define Memory trait                     [critical]  20m
  #15 Define LlmProvider trait                [critical]  30m
  #20 Define AgentEvent enum                  [high]      30m
  #12 Add TokenCounter utility                [high]      45m
  #26 Add prompt injection detection          [medium]    30m
  #27 Implement PII redaction                 [low]       20m
```

### Phase 1B — Supporting Types

```
┌─────────────────────────────────────────────────────────────┐
│  MAX PARALLELISM: 7 tasks                                   │
│  Depends on Phase 1A trait definitions                      │
└─────────────────────────────────────────────────────────────┘

  #2  Implement AgentLoop trait               [critical]  45m  → needs #1
  #6  Create ToolError type                   [high]      20m  → needs #5
  #7  Implement ToolRegistry                  [high]      30m  → needs #5
  #11 Implement SlidingWindowMemory           [high]      30m  → needs #10
  #14 Define Checkpoint trait                 [medium]    30m  → independent
  #16 Create CompletionRequest/Response       [high]      30m  → independent
  #21 Implement EventSubscriber trait         [medium]    20m  → needs #20
```

### Phase 1C — Core Implementations

```
┌─────────────────────────────────────────────────────────────┐
│  MAX PARALLELISM: 8 tasks                                   │
│  Implementation work against defined traits                 │
└─────────────────────────────────────────────────────────────┘

  #3  Refactor Swarm::run with phases         [high]      60m  → needs #1,#2
  #8  Create Tool derive macro                [medium]    60m  → needs #5,#6
  #9  Migrate AgentFunction to Tool trait     [high]      45m  → needs #5,#7
  #13 Implement context pruning               [medium]    45m  → needs #11,#12
  #17 Implement OpenAI provider client        [high]      60m  → needs #15,#16
  #22 Add OpenTelemetry integration           [medium]    60m  → needs #20,#21
  #23 Create JsonlFileSubscriber              [medium]    30m  → needs #20,#21
  #24 Add MaxIterationsError                  [high]      20m  → needs #3
```

### Phase 1D — Extended Features

```
┌─────────────────────────────────────────────────────────────┐
│  MAX PARALLELISM: 4 tasks                                   │
│  Polish and extensions                                      │
└─────────────────────────────────────────────────────────────┘

  #4  Add iteration counter and timing        [medium]    20m  → needs #3
  #18 Add Anthropic-compatible provider       [medium]    45m  → needs #15,#16
  #19 Create ProviderRegistry                 [medium]    30m  → needs #15,#17
  #25 Implement doom-loop detection           [medium]    30m  → needs #3
```

---

## 5. Tracking Progress

### 5.1 Database Commands

```bash
# View all pending tasks
sqlite3 docs/agent_context.db "SELECT id, title, priority FROM items WHERE status = 'pending'"

# Mark a task in progress
sqlite3 docs/agent_context.db "UPDATE items SET status = 'in_progress' WHERE id = 1"

# Mark complete
sqlite3 docs/agent_context.db "UPDATE items SET status = 'complete', updated_at = datetime('now') WHERE id = 1"

# Log progress
sqlite3 docs/agent_context.db "INSERT INTO entries (session_id, entry_type, content) VALUES (1, 'progress', 'Completed AgentLoopPhase enum')"

# View blocked tasks (dependencies not complete)
sqlite3 docs/agent_context.db "SELECT * FROM items WHERE status = 'pending' AND notes LIKE '%Depends on:%'"
```

### 5.2 Status Values

- `pending` — Not started
- `in_progress` — Currently being worked on
- `complete` — Finished and verified
- `blocked` — Waiting on dependency
- `deferred` — Postponed to later phase

---

## 6. File Structure (Target)

```
src/
├── lib.rs              # Crate root (update exports)
├── core.rs             # Swarm, SwarmBuilder (refactor)
├── types.rs            # Agent, Message, etc. (extend)
├── error.rs            # SwarmError (extend)
├── constants.rs        # (unchanged)
├── util.rs             # TokenCounter (add)
├── validation.rs       # (unchanged)
├── stream.rs           # (unchanged)
│
├── tool.rs             # NEW: Tool trait, ToolRegistry
├── memory.rs           # NEW: Memory trait, SlidingWindowMemory
├── provider.rs         # NEW: LlmProvider trait
├── provider/
│   ├── mod.rs          # NEW: provider module
│   ├── openai.rs       # NEW: OpenAI client
│   └── anthropic.rs    # NEW: Anthropic adapter
├── event.rs            # NEW: AgentEvent, EventSubscriber
├── observability.rs    # NEW: OpenTelemetry integration
├── checkpoint.rs       # NEW: Checkpoint trait
├── guardrails.rs       # NEW: Injection detection, PII redaction
│
└── tests/              # (extend coverage)
```

---

## 7. Notes

### 7.1 Backward Compatibility

All changes must maintain backward compatibility with existing API:
- `Swarm::run()` signature unchanged
- `Agent` struct fields preserved
- `AgentFunction` continues to work (wrapped as Tool)
- Existing tests must pass

### 7.2 Feature Flags

```toml
[features]
default = []
otel = ["opentelemetry", "opentelemetry-otlp", "tracing-opentelemetry"]
derive = ["syn", "quote", "proc-macro2"]  # For Tool derive macro
```

### 7.3 Documentation

Each new trait and struct needs:
- Rustdoc comments with examples
- Module-level documentation
- Update to README.md when significant features land

---

## 8. Changelog

| Date | Change |
|------|--------|
| 2025-03-29 | Initial decomposition — 27 atomic tasks created |
