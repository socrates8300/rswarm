# Monthly Focus Report — rswarm

## 1. Repo Model

`rswarm` is a Rust library for agent-style LLM workflows — multi-turn conversations, function/tool calling, streaming responses, XML-defined execution steps, persistence (SQLite + PostgreSQL), guardrails (injection detection, PII redaction, content policy), event hooks, circuit breakers, escalation, team formation, distributed transport, and checkpoint-based session resume. Its load-bearing surfaces are: the `Swarm::run()` entry point (the only execution path for agent loops), the `Agent`/`AgentFunction`/`Message` types that define the API contract, the `get_chat_completion()` method (the single point of LLM interaction, including SSE streaming), and the persistence traits (`SessionStore`, `EventStore`, `CheckpointStore`, `MemoryStore`). The codebase is at version 0.1.8, has 158 passing tests, clippy-clean builds, and implements a substantial feature set including observability (metrics + OTEL), but has tool abstractions (`Tool` trait + `ToolRegistry`) that are exported but not wired into the execution path, and a wire-format split where the entire library sends the legacy `functions` API format while a modern `tools` format is defined in `CompletionRequest` but never used. The code quality and test coverage suggest more maturity than the version number implies — this is genuinely in the pre-1.0 window where API decisions have outsized downstream cost.

## 2. Prioritization Rubric

I derived four dimensions from first principles for this repo at this lifecycle stage:

### Dimension 1: Wire-Protocol Survival

**Definition:** Does the library communicate with upstream APIs in a way that will continue to work as those APIs evolve?

**Why it matters here:** rswarm's entire value proposition is orchestrating LLM tool calls. Every request to every LLM flows through one method (`get_chat_completion`). If the wire format that method uses is deprecated by OpenAI, every downstream user's deployment breaks simultaneously. At 0.1.8 this is cheap to fix; at 1.0 it's a semver-major event because users will have built tooling around the request/response shape.

### Dimension 2: API Surface Coherence

**Definition:** Does the public API present a single, clear path for each operation, or does it export multiple abstractions for the same concept with different capabilities?

**Why it matters here:** rswarm exports both `AgentFunction` (closure-based, wired into `Swarm::run`) and the `Tool` trait + `ToolRegistry` (idiomatic Rust, NOT wired into `Swarm::run`). Users who choose the `Tool` trait — which looks like the "right" Rust approach — discover at runtime that their tools are never dispatched. At 0.1.8 this is a documentation/consolidation problem. At 1.0, both abstractions calcify as supported API surface and the confusion becomes permanent.

### Dimension 3: Entry-Point Reversibility

**Definition:** Can the primary API entry points be reshaped (adding parameters, changing defaults, introducing option structs) without a breaking semver-major bump?

**Why it matters here:** `Swarm::run()` takes 7 positional parameters. Every user builds against this signature. Every new feature that needs a parameter (timeout override, per-run event hooks, tool filtering) either adds an 8th positional parameter (breaking) or creates a parallel `run_with_options()` method (API surface bloat). At 0.1.8 you can migrate to a builder/option-struct pattern. At 1.0, the 7-param signature is frozen.

### Dimension 4: Feature-Flag Contract Integrity

**Definition:** Does each advertised feature flag deliver what it promises, or does it compile and then fail at runtime?

**Why it matters here:** `sqlite-vec` and `qdrant` feature flags exist in `Cargo.toml`, are documented in the README, and are mentioned in the feature-flag table. Users who `cargo add rswarm --features sqlite-vec` get a compilable crate whose `SqliteVssMemory::open()` returns a `ConfigError` at runtime. This is a trust signal — feature flags are contracts. At 0.1.8 you can either implement the adapter or remove the flag. At 1.0, the flag is a promise that breaking it would violate.

### What the rubric explicitly deprioritizes

This rubric does **not** weight: code style or clippy compliance (already clean), error handling patterns (well-structured `SwarmError` enum with `thiserror`), missing doc comments (cosmetic), dependency freshness (no known vulnerabilities), test count (158 tests is reasonable), performance profiling (no evidence of hotspots), or internal implementation details that don't leak into the public API. These are all things that a generic Rust audit would flag, but none of them change the shape of the API that users build against, and none of them will become more expensive to fix after 1.0.

## 3. Ranked Focus List

**Note on ranking changes:** The original draft ranked four items. After discussion, the maintainer confirmed: `Tool` is the permanent abstraction, break `run()`'s signature now, remove stub feature flags now, deprecate `Streamer`, swap `functions` → `tools` wire format clean. These answers cause Items 1 and 2 to collapse into a single migration, and Item 3 becomes a prerequisite for that migration. The revised ranking reflects the actual execution order.

---

### Item 1: Refactor `Swarm::run()` to a public `RunOptions` struct

**Summary:** Change `Swarm::run()` from 7 positional parameters to `(agent, messages, RunOptions)` where `RunOptions` is `#[non_exhaustive]`. This is a prerequisite for Items 2 and 3 — the tool migration and streaming consolidation both benefit from a clean entry point that doesn't carry a deprecation shim.

**Where it lives:**
- `src/core.rs::Swarm::run` (L2464-2620): the current 7-param signature
- `src/core.rs::RunOptions` (L87-92): already exists as a private struct with `model_override`, `stream`, `debug`, `max_turns`
- Every test and example that calls `swarm.run(...)`

**Concrete changes:**
1. Make `RunOptions` public, add `context_variables` to it, mark `#[non_exhaustive]`
2. Add `Default` impl for `RunOptions` (defaults: no model override, non-streaming, debug off, max_turns = 10)
3. Change `run()` signature to `run(&self, agent: Agent, messages: Vec<Message>, options: RunOptions)`
4. Remove `mut` from `agent` parameter — clone or handle instruction mutation internally (currently `run()` mutates `agent.instructions` in place at L2527, which means a second call with the same agent sees mutated state)
5. Deprecate the old 7-param signature with a thin wrapper that constructs `RunOptions` from positional args

**Rubric dimensions:**
- Entry-Point Reversibility: **Primary driver.** This is the most-called function in the library. The `#[non_exhaustive]` attribute means future fields (per-run event hooks, timeout overrides, tool filtering) can be added without breaking callers. Doing this at 0.1.8 is a minor-version migration.
- API Surface Coherence: **Secondary.** A clean `run()` signature makes the tool migration (Item 2) smoother — the new tool abstraction doesn't have to squeeze through a 7-param bottleneck.

**Why it's ranked first:** This is a prerequisite for Items 2 and 3. The tool migration (Item 2) will change how tools are registered on `Agent` — if `run()`'s signature is still 7 positional params at that point, the migration touches the signature AND the tool abstraction simultaneously, increasing risk. Doing the signature refactor first, cleanly, means the tool migration only touches tool internals.

---

### Item 2: Migrate tool abstraction to `Tool`/`ToolSchema` and wire format to `tools`

**Summary:** This collapses the original Items 1 and 2 into a single migration. Three changes in one pass: (a) wire `Tool`/`ToolRegistry` into `Swarm::run` as the dispatch path, (b) migrate the wire format from legacy `functions` to modern `tools`, (c) deprecate `AgentFunction` on `Agent` and `Streamer` in `stream.rs`.

**Where it lives:**
- `src/tool.rs::Tool` trait (L10-15), `ToolRegistry` (L264-295), `ClosureTool` (L303-347): the target abstraction
- `src/types.rs::AgentFunction` (L673-760): the abstraction being deprecated
- `src/core.rs::get_chat_completion` — streaming branch (~L1330-1460) and non-streaming branch (~L1485-1530): wire format migration
- `src/core.rs::handle_function_call` (L1547-1625): dispatch migration from `AgentFunction` to `Tool`
- `src/core.rs::handle_tool_calls_serial` (L1628-1659), `handle_tool_calls_parallel` (L1663-1688): dispatch migration
- `src/util.rs::function_to_json` (~L81-87): serialization migration
- `src/stream.rs::Streamer` (entire file): deprecate
- `src/lib.rs` exports (L72-80): update public API surface
- README "Low-Level Tool API" and "Streaming" sections: rewrite

**Concrete changes (in order):**

*Phase 2a — Wire `ToolRegistry` into `Swarm`:*
1. Add a `ToolRegistry` field to `Swarm` (alongside the existing `agent_registry`)
2. Add `SwarmBuilder::with_tool_registry(registry)` method
3. Update `handle_function_call` to dispatch through `ToolRegistry::get()` instead of iterating `agent.functions()` — fall back to `AgentFunction` for backward compat during transition
4. Update `handle_tool_calls_serial` and `handle_tool_calls_parallel` similarly
5. Add `Agent::with_tool_registry(registry)` as the new primary builder method, deprecate `Agent::with_functions()`

*Phase 2b — Migrate wire format from `functions` to `tools`:*
1. Update `function_to_json` (or add a new `tool_to_openai_tools_entry` function) to produce `{"type": "function", "function": {"name": ..., "description": ..., "parameters": ...}}`
2. In `get_chat_completion` non-streaming branch: switch from `request.with_functions(...)` to `request.with_tools(...)`
3. In `get_chat_completion` streaming branch: change `"functions": [...]` to `"tools": [...]` in the JSON body, change `"function_call": ...` to `"tool_choice": ...`
4. In `stream.rs::stream_chat`: same wire format changes (or deprecate — see Phase 2c)
5. Remove the `tool_calls → function_call` mapping shim in the non-streaming response path (~L1509-1530) — swap clean
6. Update `handle_function_call` to dispatch from `message.tool_calls()` directly instead of only checking `message.function_call()`
7. Update all tests that mock `function_call` responses to use `tool_calls` format

*Phase 2c — Deprecate `Streamer` and `AgentFunction`:*
1. Add `#[deprecated]` to `stream.rs::Streamer` with message pointing to `Swarm::run(stream=true)`
2. Remove `Streamer` from `lib.rs` re-exports (or keep with deprecation attribute)
3. Add `#[deprecated]` to `Agent::with_functions()` with message pointing to `Agent::with_tool_registry()`
4. Add `#[deprecated]` to `AgentFunction::new()` with message pointing to `Tool` trait + `ClosureTool`
5. Update README: rewrite "Low-Level Tool API" as the primary tool API, rewrite "Streaming" section to use `Swarm::run(stream=true)`, move `AgentFunction` to a "Legacy" or "Migration" section

**Rubric dimensions:**
- Wire-Protocol Survival: **Primary driver.** The `functions` format was deprecated by OpenAI in November 2023. Every request in the library currently uses it. This migration switches every request to `tools`, which is the current standard.
- API Surface Coherence: **Primary driver.** The dual `Tool`/`AgentFunction` abstraction collapses into one: `Tool` trait + `ToolRegistry`. `AgentFunction` becomes a deprecated convenience wrapper. Users have one clear path.

**Why it's second (not first):** The signature refactor (Item 1) is a prerequisite. The tool migration changes how tools are registered on `Agent` and how `run()` dispatches them. If `run()` still has 7 positional params during this migration, every test and example has to be updated twice — once for the tool change, once for the signature change. Doing the signature first means the tool migration only touches tool internals.

**Why `Streamer` deprecation is here instead of a separate item:** `Streamer` is a symptom of the same root issue — it sends `functions`-format requests and doesn't handle `tool_calls` deltas. Deprecating it is a natural part of the wire format migration. Making it a separate item would imply it needs independent attention; it doesn't — it just needs to get out of the way.

---

### Item 3: Remove `sqlite-vec` and `qdrant` feature flags and stubs

**Summary:** Remove the `sqlite-vec` and `qdrant` feature flags from `Cargo.toml`, remove the stub modules from `lib.rs` exports, and update the README to document these as planned features rather than available features.

**Where it lives:**
- `Cargo.toml` (L20-24): `sqlite-vec = []` and `qdrant = []` feature definitions
- `src/memory/sqlite_vss.rs`: `SqliteVssMemory` with `open()` returning `ConfigError`
- `src/memory/qdrant.rs`: `QdrantMemory` with `connect()` returning `ConfigError`
- `src/memory.rs` (L2-3): `pub mod qdrant; pub mod sqlite_vss;`
- `src/lib.rs`: exports `InMemoryVectorStore`, `MemoryEntry`, `RetrievalPolicy`, `VectorMemory` (these stay — only the stub adapters are removed)
- README "Optional feature flags" section and "Semantic Memory" section

**Concrete changes:**
1. Remove `sqlite-vec = []` and `qdrant = []` from `[features]` in `Cargo.toml`
2. Remove `pub mod qdrant;` and `pub mod sqlite_vss;` from `src/memory.rs`
3. Keep `src/memory/vector.rs` (the `VectorMemory` trait and `InMemoryVectorStore`) — these are solid and functional
4. Update README: remove `sqlite-vec` and `qdrant` from the feature flags table, add a "Planned Features" section noting that persistent vector backends are planned but not yet available
5. Update README "Semantic Memory" section to remove references to the stub adapters

**Rubric dimensions:**
- Feature-Flag Contract Integrity: **Primary driver.** Feature flags that compile but fail at runtime are broken contracts. Removing them is the honest move.
- API Surface Coherence: **Secondary.** The public API currently exports four vector store types but only one works. Removing the stubs leaves `InMemoryVectorStore` and the `VectorMemory` trait — a coherent surface.

**Why it's third:** This is independent of Items 1 and 2 and lower risk. It's a cleanup task that can be done in parallel with the tool migration. It doesn't block anything and nothing blocks it. It's ranked third because it scores lower on the rubric — it's a trust/readiness issue, not a correctness or reversibility issue.

---

## 4. Cut List

These are categories of issues I noticed and deliberately excluded, with the rubric-grounded reason:

- **Clippy lints across the codebase** — cut: already clean (`cargo clippy --all-targets --all-features -- -D warnings` passes with zero warnings). Not a rubric dimension issue.

- **Missing doc comments on internal functions** — cut: cosmetic, doesn't affect any rubric dimension. The public API surface has reasonable docs.

- **`VALID_API_URL_PREFIXES` default allowlist is narrow** — cut: the builder provides `with_valid_api_url_prefixes()` to override defaults. Users with self-hosted providers can configure around this. Doesn't score on any rubric dimension.

- **`ContextVariables` is `HashMap<String, String>` (stringly-typed tool arguments)** — cut: this is a deliberate design choice that enables the `AgentFunction` closure pattern. Changing it would be a different library design, not a maintenance fix. Doesn't score on any rubric dimension at this stage.

- **`max_depth` type mismatch (`Option<usize>` in `RuntimeLimits` vs `u32` in `BudgetEnforcer`)** — cut: internal type inconsistency that doesn't leak into the public API in a way users would encounter. Doesn't score on any rubric dimension.

- **`LlmProvider::stream()` returns "not yet implemented" error** — cut: the trait method exists but the streaming path uses custom code in `core.rs`. The provider trait is extensible for future providers. Fixing this is internal plumbing, not a user-facing API issue. Subsumed by Item 2's streaming consolidation.

- **`handle_function_result` is a pass-through (just returns `Ok(result)`)** — cut: this is a hook point for future customization, not a bug. It scores zero on all rubric dimensions.

- **`Agent` serialization uses a transport DTO instead of direct serde** — cut: works correctly, tested via roundtrip tests. Internal implementation detail.

- **SQLite persistence uses `Mutex<Connection>` wrapped in async** — cut: works correctly for the stated use case (single-writer). No evidence of contention issues.

- **`SlidingWindowMemory` uses word-count token estimation** — cut: documented as approximate, suitable for the stated use case. Not a public API issue.

- **Error variant overlap (`ApiError` vs `ReqwestError`, `XmlError` vs `XmlParseError`)** — cut: may cause confusion but doesn't break any rubric dimension. Could be cleaned up but isn't load-bearing.

- **`with_retry` utility exists but `core.rs` reimplements retry logic inline** — cut: internal code duplication that doesn't affect the public API. Could be DRYed up but isn't user-facing.

- **No integration tests against real or mock LLM endpoints for the full `Swarm::run` path** — cut: the tests use wiremock for streaming and mock responses. Coverage is reasonable. This is a test quality improvement, not a rubric-dimension issue.

- **`run()` mutates `agent.instructions` in place** — cut as standalone item: noted in Item 1 as part of the signature refactor (remove `mut` from agent param). Not a separate work item.

## 5. Confirmed Decisions and Sequencing

The maintainer confirmed the following decisions, which shaped the revised ranking:

| # | Question | Answer |
|---|----------|--------|
| 1 | Primary tool abstraction | `Tool` trait is the future; `AgentFunction` gets deprecated |
| 2 | Breaking `run()` signature | Yes, break it now |
| 3 | Stub feature flags | Remove them now |
| 4 | `Streamer` | Deprecate it |
| 5 | `functions` → `tools` migration | Swap clean, no backward-compat shim |

### Sequencing

The three ranked items have a dependency:

```
Item 1 (signature refactor)
  └──► Item 2 (tool abstraction + wire format + Streamer deprecation)
         (Item 3 is independent, can run in parallel with either)
```

**Do Item 1 first.** It's small, mechanical, and de-risks Item 2. The tool migration changes how tools are registered and dispatched — if `run()`'s signature is already clean, the tool migration doesn't have to touch the signature at the same time.

**Item 3 can happen anytime.** It's independent of the other two. If you want a quick win to start the month, do Item 3 on day one — it's 30 minutes of work and immediately improves the trust signal.

**Item 2 is the bulk of the month's effort.** It has three phases (wire `ToolRegistry` into dispatch, migrate wire format, deprecate old abstractions). Each phase is testable independently. The phases should be done in order — wiring `ToolRegistry` first means the wire format migration has a clean dispatch path to target.

### What changes if the `Tool` migration reveals unexpected complexity

The biggest risk in Item 2 is Phase 2a (wiring `ToolRegistry` into dispatch). The `handle_function_call` method does more than just call the function — it validates arguments against the schema, converts `InvocationArgs` to `ContextVariables`, handles `accepts_context_variables`, and manages the `ResultType` enum (Value, Agent, ContextVariables, Termination). The `Tool::execute()` method takes `InvocationArgs` and returns `Result<Value, ToolError>` — it doesn't have the `ResultType` dispatch semantics.

This means the `Tool` trait may need a richer return type to support agent handoff and context variable injection. If that's the case, the migration is more than mechanical — it's a design decision about whether `Tool::execute()` should return `ResultType` or whether the `Swarm` should handle the `ResultType` semantics separately from tool execution.

Watch for this during Phase 2a. If it surfaces, the right move is: keep `Tool::execute()` returning `Result<Value, ToolError>`, and add a `ToolResult` enum that wraps the return with handoff/context/termination metadata at the `Swarm` dispatch layer. Don't change the `Tool` trait's return type — that would push framework semantics into the tool implementation.
