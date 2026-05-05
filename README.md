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

- `with_function_call_policy(...)`
- `with_tool_call_execution(...)`
- `with_expected_response_fields(...)`
- `with_capabilities(...)`

Tools are registered on the `Swarm`, not on the `Agent` — see
[Function Calling](#function-calling). `Agent::with_functions(...)` is
still available for migration but is `#[deprecated]`.

Instruction modes:

- `Instructions::Text(String)`
- `Instructions::Function(Arc<dyn Fn(ContextVariables) -> String + Send + Sync>)`

## Function Calling

Tools are defined as types that implement the `Tool` trait, registered on
a `ToolRegistry`, and attached to the `Swarm` via
`SwarmBuilder::with_tool_registry(...)`. The registry is advertised to the
LLM as the `tools` field in the OpenAI chat-completions wire format and
dispatched on `tool_call` responses without any per-agent wiring.

```rust
use async_trait::async_trait;
use rswarm::{
    Agent, FunctionCallPolicy, Instructions, InvocationArgs, Message, RunOptions,
    Swarm, Tool, ToolCallExecution, ToolError, ToolRegistry,
};
use serde_json::{json, Value};

struct GetWeather;

#[async_trait]
impl Tool for GetWeather {
    fn name(&self) -> &str { "get_weather" }
    fn description(&self) -> &str { "Return a short weather summary for a city" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "city": { "type": "string" } },
            "required": ["city"],
            "additionalProperties": false,
        })
    }
    async fn execute(&self, args: InvocationArgs) -> Result<Value, ToolError> {
        let city = args
            .as_object()
            .and_then(|m| m.get("city"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        Ok(Value::String(format!("Sunny in {city}")))
    }
}

let agent = Agent::new(
    "weather-bot",
    "gpt-4o",
    Instructions::Text("Use tools when needed.".to_string()),
)?
.with_function_call_policy(FunctionCallPolicy::Auto)
.with_tool_call_execution(ToolCallExecution::Parallel);

let mut registry = ToolRegistry::new();
registry.register(GetWeather);

let swarm = Swarm::builder()
    .with_api_key(std::env::var("OPENAI_API_KEY")?)
    .with_agent(agent.clone())
    .with_tool_registry(registry)
    .build()?;

let response = swarm
    .run(
        agent,
        vec![Message::user("Weather in Paris?")?],
        RunOptions::new().with_max_turns(5),
    )
    .await?;
```

### Marker conventions for non-`Value` results

`Tool::execute` returns `Result<Value, ToolError>`, but a tool can also
trigger an agent handoff, update run-level context variables, or
terminate the run by emitting a single-key marker object:

| Marker key                  | Payload                                            | Effect                                        |
|-----------------------------|----------------------------------------------------|-----------------------------------------------|
| `__rswarm_agent_handoff`    | `"agent_name"` (must be registered on the `Swarm`) | Switches the active agent for the next turn  |
| `__rswarm_context_update`   | object of `{ "key": "value", ... }`                | Merges into the run's `ContextVariables`     |
| `__rswarm_termination`      | serialized `TerminationReason`                     | Ends the run with the given reason           |

Anything else (string, number, object, array) is treated as a normal
tool result and forwarded back to the model as a string.

```rust
use serde_json::json;
// In a Tool::execute body:
Ok(json!({ "__rswarm_agent_handoff": "specialist_agent" }))
```

### Notes

- Parameter schemas must be JSON Schema objects with root `"type": "object"`.
- Tool arguments are validated with `jsonschema`, not a hand-rolled subset.
- `ToolCallExecution::Serial` threads context updates from one call into the next.
- `ToolCallExecution::Parallel` executes calls independently and preserves per-tool success/failure reporting.
- A `Tool` registered on the `Swarm` shadows any `AgentFunction` of the
  same name on the agent, both at advertisement time and during dispatch.

### Migrating from `AgentFunction`

`AgentFunction::new(...)` and `Agent::with_functions(...)` are
`#[deprecated]` since 0.1.9 and routed through the same dispatch path
as the `Tool` trait, so existing code continues to work. To migrate
without rewriting the closure body, wrap the `AgentFunction` with
`ClosureTool::from_agent_function(...)`:

```rust
# use rswarm::{ClosureTool, ToolRegistry};
# fn build(my_agent_fn: rswarm::AgentFunction) {
let mut registry = ToolRegistry::new();
// The wrapped `AgentFunction`'s description is carried through by
// default; call `.with_description(...)` to override or to supply one
// when the source `AgentFunction` did not set one.
registry.register(ClosureTool::from_agent_function(my_agent_fn));
// ... swarm.builder().with_tool_registry(registry) ...
# }
```

`ClosureTool` emits the marker conventions for `ResultType::Agent`,
`ResultType::ContextVariables`, and `ResultType::Termination`, so a
migrated closure has the same observable behavior as before.

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

Licensed under the [MIT License](LICENSE).
