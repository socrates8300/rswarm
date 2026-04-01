# rswarm

`rswarm` is a Rust library for agent-style LLM workflows: multi-turn conversations, function and tool calling, streaming responses, XML-defined execution steps, persistence, guardrails, and event hooks.

## Is rswarm Right for Your Project?

`rswarm` is aimed at **Rust teams building production backend services** that need LLM-driven logic as a first-class component — not as an afterthought bolted on at the edge.

**It's a strong fit if:**

- You're building in Rust and want LLM workflows to benefit from the same type safety, memory safety, and performance guarantees as the rest of your stack.
- Your use case involves **agentic control flow**: routing between specialized agents based on capability, iterating on a task across multiple turns, or executing different tools based on model output.
- You need **production hardening** out of the box: persistent session history, event auditing, circuit breakers for provider or tool failures, escalation policies, content guardrails, and OpenTelemetry tracing.
- Your workflow involves **parallel or serial tool execution** where the model selects from a registered set of Rust functions and you want schema-validated arguments, not stringly-typed JSON parsing scattered through your codebase.
- You want to swap between **SQLite (embedded) and PostgreSQL (remote)** persistence without touching application logic.

**It's probably not the right fit if:**

- Your primary stack is Python, TypeScript, or another language — the library is Rust-only.
- You need a simple one-shot LLM call without multi-turn state or tooling; a direct API client is less overhead at that scale.
- You need a persistent vector search backend today — the `sqlite-vec` and `qdrant` adapters are reserved but not yet implemented; in-memory semantic search is the current ceiling.
- Your API provider doesn't expose an OpenAI-compatible chat completions endpoint.

**The mental model:** rswarm sits between "call the OpenAI API yourself" and "use a full LLM application framework." It handles the protocol complexity, retry logic, schema validation, and persistence that appear in every production deployment, while keeping the surface area narrow enough to compose cleanly with the rest of a Rust service. It is not a RAG pipeline, a prompt management system, or a model-evaluation harness — it covers the runtime orchestration layer only.

---

The current workspace passes:

- `cargo fmt --all --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo doc --no-deps --all-features`
- `cargo test --workspace --all-features`

## What It Covers

- Agent construction with static or dynamic instructions
- Multi-turn `Swarm::run(...)` conversations
- Function calling with serial or parallel tool execution
- Streaming responses through `rswarm::stream::Streamer`
- Structured response checks and JSON Schema-backed tool argument validation
- SQLite and PostgreSQL persistence backends
- Event subscribers, circuit breakers, escalation, and guardrails
- In-memory semantic memory plus feature-gated vector backends

## Installation

Add the library:

```bash
cargo add rswarm
```

For most applications you will also want:

```bash
cargo add tokio --features macros,rt-multi-thread
cargo add dotenvy
cargo add serde_json
```

If you plan to use the streaming API shown below:

```bash
cargo add futures-util
```

Optional feature flags:

- `postgres`: PostgreSQL persistence
- `postgres-tls`: PostgreSQL persistence with rustls + native roots
- `metrics-export`: Prometheus metrics exporter
- `otel`: OpenTelemetry tracing export
- `sqlite-vec`: reserved feature; adapter currently returns a configuration error
- `qdrant`: reserved feature; adapter currently returns a configuration error

Example:

```bash
cargo add rswarm --features postgres,postgres-tls
```

## Configuration

Environment variables:

- `OPENAI_API_KEY`: required unless passed directly to `Swarm::builder().with_api_key(...)`
- `OPENAI_API_URL`: optional override for the chat-completions endpoint

Default API URL:

```text
https://api.openai.com/v1/chat/completions
```

The examples crate also uses:

- `OPENAI_MODEL`: optional, defaults to `gpt-4o`

## Quick Start

`Swarm::run(...)` requires a non-empty message history. Use the message constructors instead of struct literals.

```rust
use rswarm::{Agent, ContextVariables, Instructions, Message, Swarm};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let agent = Agent::new(
        "assistant",
        "gpt-4o",
        Instructions::Text("You are a concise Rust assistant.".to_string()),
    )?;

    let swarm = Swarm::builder()
        .with_agent(agent.clone())
        .build()?;

    let response = swarm
        .run(
            agent,
            vec![Message::user("Give me a one-sentence overview of ownership.")?],
            ContextVariables::new(),
            None,
            false,
            false,
            5,
        )
        .await?;

    for message in &response.messages {
        if let Some(content) = message.content() {
            println!("{}: {}", message.role(), content);
        }
    }

    Ok(())
}
```

## Defining Agents

Create agents with `Agent::new(...)` and then opt into additional behavior with builder-style methods:

```rust
use rswarm::{Agent, FunctionCallPolicy, Instructions, ToolCallExecution};

let agent = Agent::new(
    "router",
    "gpt-4o",
    Instructions::Text("Route requests to the right tool.".to_string()),
)?
.with_function_call_policy(FunctionCallPolicy::Auto)
.with_tool_call_execution(ToolCallExecution::Parallel)
.with_capabilities(vec!["routing".to_string(), "triage".to_string()]);
```

Relevant agent APIs:

- `with_functions(...)`
- `with_function_call_policy(...)`
- `with_tool_call_execution(...)`
- `with_expected_response_fields(...)`
- `with_capabilities(...)`

Instruction modes:

- `Instructions::Text(String)`
- `Instructions::Function(Arc<dyn Fn(ContextVariables) -> String + Send + Sync>)`

## Function Calling

`AgentFunction` is the main application-level tool/function abstraction used during `Swarm::run(...)`.

```rust
use rswarm::{
    Agent, AgentFunction, ContextVariables, FunctionCallPolicy, Instructions, ResultType,
    ToolCallExecution,
};
use serde_json::json;
use std::sync::Arc;

let weather = AgentFunction::new(
    "get_weather",
    Arc::new(|args: ContextVariables| {
        Box::pin(async move {
            let city = args
                .get("city")
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());
            Ok(ResultType::Value(format!("Sunny in {city}")))
        })
    }),
    true,
)?
.with_description("Return a short weather summary for a city")
.with_parameters_schema(json!({
    "type": "object",
    "properties": {
        "city": { "type": "string" }
    },
    "required": ["city"],
    "additionalProperties": false
}))?;

let agent = Agent::new(
    "weather-bot",
    "gpt-4o",
    Instructions::Text("Use tools when needed.".to_string()),
)?
.with_functions(vec![weather])
.with_function_call_policy(FunctionCallPolicy::Auto)
.with_tool_call_execution(ToolCallExecution::Parallel);
```

Notes:

- Parameter schemas must be JSON Schema objects with root `"type": "object"`.
- Tool arguments are validated with `jsonschema`, not a hand-rolled subset.
- `accepts_context_variables = true` passes validated arguments into the handler as `ContextVariables`.
- `ToolCallExecution::Serial` threads context updates from one call into the next.
- `ToolCallExecution::Parallel` executes calls independently and preserves per-tool success/failure reporting.

## Low-Level Tool API

If you want a lower-level tool abstraction outside the `AgentFunction` flow, the crate also exposes:

- `Tool`
- `ClosureTool`
- `ToolRegistry`
- `InvocationArgs`
- `ToolSchema`
- `ToolCallSpec`

Use this layer when you want explicit tool registration/execution without relying on `AgentFunction`.

## Messages

Use constructors instead of field access:

```rust
use rswarm::{FunctionCall, Message, ToolCall};

let user = Message::user("hello")?;
let assistant = Message::assistant("hi")?;
let function = Message::function("lookup_user", "{\"id\":42}")?;
let tool_result = Message::tool_result("call_123", "{\"ok\":true}")?;

let tool_call = ToolCall::new("call_123", FunctionCall::new("lookup_user", "{\"id\":42}")?)?;
let assistant_with_tools = Message::assistant_tool_calls(vec![tool_call])?;
```

Important message constraints:

- `Swarm::run(...)` rejects an empty `messages` vector
- assistant messages must contain exactly one of `content`, `function_call`, or `tool_calls`
- tool messages must include `tool_call_id`

## Streaming

Use `rswarm::stream::Streamer` for incremental output:

```rust
use futures_util::StreamExt;
use rswarm::{stream::Streamer, Agent, ContextVariables, Instructions, Message, Swarm};

let agent = Agent::new(
    "assistant",
    "gpt-4o",
    Instructions::Text("Respond in short streaming chunks.".to_string()),
)?;

let swarm = Swarm::builder()
    .with_api_key(std::env::var("OPENAI_API_KEY")?)
    .with_agent(agent.clone())
    .build()?;

let streamer = Streamer::new(
    swarm.client().clone(),
    swarm.api_key().clone(),
    swarm.config().api_url().to_string(),
);

let history = vec![Message::user("Stream a greeting.")?];
let mut stream = streamer.stream_chat(
    &agent,
    &history,
    &ContextVariables::new(),
    None,
    false,
);

while let Some(item) = stream.next().await {
    let message = item?;
    if let Some(content) = message.content() {
        print!("{content}");
    }
}
```

## Structured Responses

If you expect a JSON-shaped answer, you can require fields up front:

```rust
use rswarm::{Agent, Instructions};

let agent = Agent::new(
    "structured",
    "gpt-4o",
    Instructions::Text("Respond with JSON only.".to_string()),
)?
.with_expected_response_fields(vec![
    "answer".to_string(),
    "confidence".to_string(),
])?;
```

## XML-Defined Execution Steps

`rswarm` can extract and execute XML-defined steps embedded in the instruction text. The `Swarm::run(...)` path handles parsing and execution automatically.

Example shape:

```xml
<steps>
  <step number="1" action="run_once">
    <prompt>Summarize the request.</prompt>
  </step>
  <step number="2" action="loop" agent="assistant">
    <prompt>Continue until the task is complete.</prompt>
  </step>
</steps>
```

See [`rswarm_examples/prompt.txt`](rswarm_examples/prompt.txt) for a real example.

## Persistence

SQLite:

```rust
use rswarm::{SqliteStore, Swarm};

let store = SqliteStore::open("rswarm.db")?;
let swarm = Swarm::builder()
    .with_api_key(std::env::var("OPENAI_API_KEY")?)
    .with_persistence_backend(store)
    .build()?;
```

PostgreSQL:

```rust
use rswarm::PostgresStore;

// Localhost / Unix-socket only, because this path uses NoTls.
let local_store = PostgresStore::connect("postgres://localhost/rswarm").await?;
```

For remote PostgreSQL, use TLS:

```rust
use rswarm::PostgresStore;

let store = PostgresStore::connect_with_native_roots(
    "postgres://user:pass@db.example.com/rswarm",
)
.await?;
```

This helper requires the `postgres-tls` feature.

Persistence backends cover sessions, events, checkpoints, and memory records.

## Semantic Memory

Available today:

- `InMemoryVectorStore`
- `RetrievalPolicy`
- `VectorMemory`

Current status of feature-gated adapters:

- `sqlite-vec`: feature exists, persistent adapter currently returns a configuration error
- `qdrant`: feature exists, adapter currently returns a configuration error

Use `InMemoryVectorStore` for development and small deployments until those adapters land.

## Events, Guardrails, and Runtime Controls

The builder supports:

- `with_subscriber(...)`
- `with_runtime_limits(...)`
- `with_content_policy(...)`
- `with_injection_policy(...)`
- `with_redaction_policy(...)`
- `with_redaction_threshold(...)`
- `with_escalation_config(...)`
- `with_provider_circuit_breaker(...)`
- `with_tool_circuit_breaker(...)`

These are useful for observability, compliance, and production hardening.

## Examples

Runnable example crate:

```bash
cargo run -p rswarm_examples
```

The example crate uses `dotenvy`, reads [`rswarm_examples/prompt.txt`](rswarm_examples/prompt.txt), and requires a local Chrome/Chromium install for the docs browser tool.

See:

- [`rswarm_examples/src/main.rs`](rswarm_examples/src/main.rs)
- [`rswarm_examples/README.md`](rswarm_examples/README.md)

## Development Workflow

Useful commands:

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo doc --no-deps --all-features
cargo test --workspace --all-features
cargo audit
```

## Current Caveats

- `Swarm::run(...)` requires at least one input message
- the vector database adapters behind `sqlite-vec` and `qdrant` are not implemented yet
- `Agent` and `Message` use constructors/builders; their internal fields are not public API
- remote PostgreSQL connections should use TLS helpers, not `PostgresStore::connect(...)`

## License

Licensed under either:

- MIT
- Apache-2.0
