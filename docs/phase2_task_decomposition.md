# Phase 2: Persistence and Production Hardening — Task Decomposition

> **Source**: PRD Section 11.2 — Persistence and Production Hardening  
> **Related PRD Sections**: 3.2, 3.3, 6.3, 6.4, 7.1-7.3  
> **Reference**: `docs/phase1_task_decomposition.md`  
> **Created**: 2026-03-30  
> **Database**: `docs/agent_context.db`

---

## 1. Overview

### 1.1 Phase 2 Goals (from PRD)

Deliver production-ready hardening on top of the Phase 1 harness:
- **Durable state** with **SQLite-backed structured persistence**
- **Pluggable memory backends** for local vector retrieval and future remote stores
- **Checkpointing and resume** for interrupted sessions
- **Comprehensive guardrails** for prompt injection, PII, policy enforcement, and runaway execution
- **Operational observability** with metrics export, dashboards, alerts, and replayable traces

### 1.2 Success Criteria (PRD 11.2)

- [ ] A completed run can be **persisted, resumed, and audited**
- [ ] The default durable backend works with **SQLite only**, with no external service required
- [ ] All safety limits are **configurable, observable, and enforced before failure cascades**
- [ ] Tool/provider failures degrade through **timeouts, quotas, and circuit breakers**
- [ ] Metrics are exportable to **Prometheus/OpenTelemetry** and have **Grafana-ready dashboards**
- [ ] Incident response has **documented alert rules and runbooks**

---

## 2. Current State Assessment

### 2.1 What Exists

| Component | Location | Status |
|-----------|----------|--------|
| In-memory memory trait | `src/memory.rs` | ✅ Functional |
| SlidingWindowMemory | `src/memory.rs` | ✅ In-memory only |
| Event model and subscribers | `src/event.rs` | ✅ Functional |
| Prompt injection and PII helpers | `src/guardrails.rs` | ✅ Basic pattern matching |
| Max loop iterations | `src/types.rs`, `src/core.rs` | ✅ Basic limit support |
| Typed request/message/tool validation | `src/types.rs`, `src/tool.rs`, `src/validation.rs` | ✅ Functional |
| Local example app | `rswarm_examples/` | ✅ Builds against local crate |

### 2.2 Gaps to Address

| Gap | PRD Reference | Priority |
|-----|---------------|----------|
| Durable structured storage | 3.2.2, 11.2.1 | Critical |
| Checkpointing and session resume | 3.3.1, 3.3.2 | Critical |
| Pluggable vector retrieval backend | 3.2.1, 11.2.1 | High |
| Event-driven memory hooks | 3.2.3 | High |
| Configurable runtime budgets | 7.3.1-7.3.3, 11.2.2 | Critical |
| Tool/provider circuit breakers | 7.3.3, 11.2.2 | High |
| Policy-grade guardrails and audit hooks | 7.1, 7.2, 11.2.2 | High |
| Prometheus/OpenTelemetry export hardening | 6.3, 11.2.3 | High |
| Dashboards, alert rules, and runbooks | 6.4, 11.2.3 | Medium |

---

## 3. Task Breakdown

### 3.1 Durable Storage and Checkpointing

| ID | Title | Priority | Dependencies | Location |
|----|-------|----------|--------------|----------|
| #28 | Define persistence backend traits for sessions, events, checkpoints, and memories | critical | none | `src/persistence.rs` (new) |
| #29 | Create SQLite schema and migration runner | critical | #28 | `src/persistence/sqlite.rs` (new), `migrations/` (new) |
| #30 | Implement SqliteSessionStore for runs, messages, tool calls, and summaries | high | #28, #29 | `src/persistence/sqlite.rs` |
| #31 | Define versioned CheckpointData and serialization envelope | critical | #28 | `src/checkpoint.rs` (new) |
| #32 | Persist checkpoints from loop boundaries | critical | #31 | `src/core.rs`, `src/phase.rs` |
| #33 | Add resume API with checkpoint compatibility validation | critical | #31, #32 | `src/core.rs`, `src/checkpoint.rs` |
| #34 | Add retention, pruning, and archival policy for persisted sessions | medium | #29, #30 | `src/persistence/sqlite.rs`, `src/config.rs` (new or extend) |

#### Task #28: Define persistence backend traits

**Depends on**: none  
**Acceptance criteria**:
- Define traits for session metadata, event append/read, checkpoint save/load, and memory persistence
- Keep the default backend single-process and async-friendly
- Backend interfaces are storage-agnostic so SQLite is an implementation, not a hardcoded dependency

**Estimate**: medium (45 min)

---

#### Task #29: Create SQLite schema and migration runner

**Depends on**: #28  
**Acceptance criteria**:
- Create SQLite DDL for sessions, messages, events, checkpoints, memories, and migrations
- Migration runner applies idempotent versioned migrations
- SQLite is the default embedded persistence option with zero required external services

**Estimate**: medium (60 min)

---

#### Task #30: Implement SqliteSessionStore

**Depends on**: #28, #29  
**Acceptance criteria**:
- Store and query session metadata, message history, tool call history, and loop outcomes
- Support reads by session id, date range, and trace id
- Add indexes for common filters used by replay and debugging workflows

**Estimate**: medium (60 min)

---

#### Task #31: Define versioned CheckpointData

**Depends on**: #28  
**Acceptance criteria**:
- Versioned checkpoint envelope includes messages, context variables, current agent, loop counters, and pending work
- Serialization/deserialization is explicit and migration-ready
- Checkpoint format is suitable for persistence and resume validation

```rust
pub struct CheckpointEnvelope {
    pub version: u32,
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    pub payload: CheckpointData,
}
```

**Estimate**: medium (45 min)

---

#### Task #32: Persist checkpoints from loop boundaries

**Depends on**: #31  
**Acceptance criteria**:
- Checkpoints can be emitted per iteration, per N iterations, or on explicit boundaries
- Checkpointing hooks are tied to phase or loop transitions, not ad hoc call sites
- Failures to checkpoint are observable and classified without corrupting in-memory state

**Estimate**: medium (60 min)

---

#### Task #33: Add resume API with compatibility validation

**Depends on**: #31, #32  
**Acceptance criteria**:
- `Swarm` can resume a session from a persisted checkpoint
- Resume validates checkpoint version, agent availability, and required state
- Incompatible or corrupt checkpoints fail with structured errors and graceful fallback

**Estimate**: medium (60 min)

---

#### Task #34: Add retention, pruning, and archival policy

**Depends on**: #29, #30  
**Acceptance criteria**:
- Configurable retention policy by age, status, or storage budget
- Archive path keeps replay/debugging data while pruning hot tables
- Maintenance operations are documented and safe to run repeatedly

**Estimate**: small (30 min)

---

### 3.2 Vector Memory and Event-Driven Updates

| ID | Title | Priority | Dependencies | Location |
|----|-------|----------|--------------|----------|
| #35 | Define VectorMemory trait and retrieval policy types | high | #28 | `src/memory/vector.rs` (new) |
| #36 | Implement sqlite-vss-backed vector memory adapter | high | #35, #29 | `src/memory/sqlite_vss.rs` (new) |
| #37 | Add optional Qdrant adapter behind feature flag | medium | #35 | `src/memory/qdrant.rs` (new), `Cargo.toml` |
| #38 | Emit event-driven memory hooks for summaries, tool results, and explicit writes | high | #28, #30, #35 | `src/core.rs`, `src/event.rs`, `src/memory/` |

#### Task #35: Define VectorMemory trait

**Depends on**: #28  
**Acceptance criteria**:
- Abstract embedding-backed store/retrieve/search operations
- Retrieval policy captures top-k, score threshold, and recency weighting
- Trait supports embedded and remote implementations without changing caller code

**Estimate**: medium (45 min)

---

#### Task #36: Implement sqlite-vss adapter

**Depends on**: #35, #29  
**Acceptance criteria**:
- Embedded vector retrieval works with SQLite + sqlite-vss or equivalent local extension
- Memory insert and semantic search APIs are tested
- Fallback behavior is clear when vector extension support is unavailable

**Estimate**: medium (75 min)

---

#### Task #37: Add Qdrant adapter behind feature flag

**Depends on**: #35  
**Acceptance criteria**:
- `qdrant` feature flag gates remote vector dependency
- Adapter implements the same retrieval interface as the local backend
- Configuration supports host, collection, auth, and timeout settings

**Estimate**: medium (60 min)

---

#### Task #38: Emit event-driven memory hooks

**Depends on**: #28, #30, #35  
**Acceptance criteria**:
- Memory persistence is triggered from lifecycle events, not polling loops
- Conversation completion, tool results, and explicit memory writes flow through a shared hook surface
- Hook behavior is observable through structured events

**Estimate**: medium (60 min)

---

### 3.3 Guardrails, Limits, and Policy Enforcement

| ID | Title | Priority | Dependencies | Location |
|----|-------|----------|--------------|----------|
| #39 | Extend runtime config with token, time, depth, and quota limits | critical | none | `src/types.rs`, `src/core.rs` |
| #40 | Implement cumulative budget enforcer for iterations, tokens, and wall-clock time | critical | #39 | `src/core.rs`, `src/validation.rs` |
| #41 | Add tool and provider circuit breaker state machines | high | #39 | `src/tool.rs`, `src/provider.rs`, `src/core.rs` |
| #42 | Make prompt injection handling policy-driven and auditable | high | none | `src/guardrails.rs`, `src/event.rs` |
| #43 | Add data classification tags and redaction pipeline for logs/events | high | #42 | `src/guardrails.rs`, `src/event.rs` |
| #44 | Add content policy hook and violation audit path | medium | #42 | `src/guardrails.rs`, `src/core.rs` |
| #45 | Add output verification for tool calls and structured LLM responses | high | #41 | `src/validation.rs`, `src/provider.rs`, `src/tool.rs` |
| #46 | Add heuristic escalation triggers for hallucinations and repeated failures | medium | #40, #45 | `src/core.rs`, `src/guardrails.rs` |

#### Task #39: Extend runtime config with limits

**Depends on**: none  
**Acceptance criteria**:
- `SwarmConfig` supports per-task, per-session, and per-agent limits
- Limits cover iterations, recursion/depth, token budgets, tool call quotas, and wall-clock time
- Defaults are safe and overridable

**Estimate**: medium (45 min)

---

#### Task #40: Implement cumulative budget enforcer

**Depends on**: #39  
**Acceptance criteria**:
- Enforcement occurs before runaway execution, not after the fact
- Exhaustion produces structured errors and partial-response behavior where appropriate
- Token, iteration, and elapsed-time counters are exported as observable metrics

**Estimate**: medium (60 min)

---

#### Task #41: Add tool and provider circuit breakers

**Depends on**: #39  
**Acceptance criteria**:
- Consecutive failure thresholds open a breaker for tools and providers
- Breakers support closed, open, and half-open states with reset policy
- Breaker state changes emit events and are included in debug output

```rust
pub enum CircuitState {
    Closed,
    Open { opened_at: Instant },
    HalfOpen,
}
```

**Estimate**: medium (60 min)

---

#### Task #42: Make prompt injection handling policy-driven

**Depends on**: none  
**Acceptance criteria**:
- Support explicit actions: `warn`, `sanitize`, `reject`
- Policy is configurable at runtime
- All detections are auditable through structured events and persisted traces

**Estimate**: small (30 min)

---

#### Task #43: Add data classification and redaction pipeline

**Depends on**: #42  
**Acceptance criteria**:
- Sensitive fields can be tagged or inferred before writing events/logs
- Redaction applies consistently to logs, persisted events, and replay output
- Policies distinguish redaction, masking, and drop-on-write behaviors

**Estimate**: medium (45 min)

---

#### Task #44: Add content policy hook

**Depends on**: #42  
**Acceptance criteria**:
- Hook surface exists for request/response policy checks
- Violations produce structured audit events
- Default implementation is simple and overridable

**Estimate**: small (30 min)

---

#### Task #45: Add output verification

**Depends on**: #41  
**Acceptance criteria**:
- Structured outputs and tool call parameters are validated before execution
- Validation failures feed back through retry/escalation paths instead of panicking
- Unknown tools, wrong parameter types, and malformed structured responses are classified cleanly

**Estimate**: medium (60 min)

---

#### Task #46: Add heuristic escalation triggers

**Depends on**: #40, #45  
**Acceptance criteria**:
- Detect repeated failures, non-existent tool calls, and circular reasoning patterns
- Escalation triggers are observable and configurable
- Escalation path can stop execution, request human review, or inject a continuation warning

**Estimate**: medium (45 min)

---

### 3.4 Observability, Metrics, and Operations

| ID | Title | Priority | Dependencies | Location |
|----|-------|----------|--------------|----------|
| #47 | Harden OpenTelemetry export and trace propagation | high | #30, #41 | `src/observability.rs` (new), `src/event.rs` |
| #48 | Add Prometheus metrics endpoint and registry wiring | high | #47 | `src/observability.rs`, `src/bin/metrics.rs` (new or feature-gated) |
| #49 | Create Grafana dashboards, alert rules, and runbooks | medium | #47, #48 | `deploy/grafana/` (new), `deploy/prometheus/` (new), `docs/runbooks/` (new) |

#### Task #47: Harden OpenTelemetry export

**Depends on**: #30, #41  
**Acceptance criteria**:
- Traces, logs, and metrics can be exported with environment-driven configuration
- Span context propagates through tool calls and provider requests
- Correlation IDs remain available for future multi-agent scenarios

**Estimate**: medium (60 min)

---

#### Task #48: Add Prometheus metrics endpoint

**Depends on**: #47  
**Acceptance criteria**:
- Counters/histograms/gauges cover latency, token usage, tool outcomes, breaker states, and guardrail events
- Metrics endpoint or exporter can be enabled without changing core business logic
- Naming and labels are stable enough for dashboards and alert rules

**Estimate**: medium (60 min)

---

#### Task #49: Create dashboards, alerts, and runbooks

**Depends on**: #47, #48  
**Acceptance criteria**:
- Grafana dashboard JSON or provisioning files exist for core agent metrics
- Alerting rules cover latency spikes, error-rate jumps, breaker openings, and budget exhaustion
- Runbooks document common failure modes and first-response actions

**Estimate**: medium (75 min)

---

## 4. Execution Order

### Phase 2A — Persistence Foundations

```
┌─────────────────────────────────────────────────────────────┐
│  MAX PARALLELISM: 4 tasks                                   │
│  Establish durable interfaces before concrete backends      │
└─────────────────────────────────────────────────────────────┘

  #28 Define persistence backend traits       [critical]  45m
  #31 Define versioned CheckpointData         [critical]  45m
  #35 Define VectorMemory trait               [high]      45m
  #39 Extend runtime config with limits       [critical]  45m
```

### Phase 2B — Embedded Default Backend

```
┌─────────────────────────────────────────────────────────────┐
│  MAX PARALLELISM: 5 tasks                                   │
│  Build the SQLite-first default production path             │
└─────────────────────────────────────────────────────────────┘

  #29 Create SQLite schema and migrations     [critical]  60m  → needs #28
  #30 Implement SqliteSessionStore            [high]      60m  → needs #28,#29
  #32 Persist checkpoints from loop edges     [critical]  60m  → needs #31
  #33 Add resume API and validation           [critical]  60m  → needs #31,#32
  #38 Emit event-driven memory hooks          [high]      60m  → needs #28,#30,#35
```

### Phase 2C — Guardrails and Enforcement

```
┌─────────────────────────────────────────────────────────────┐
│  MAX PARALLELISM: 6 tasks                                   │
│  Turn the harness into a bounded, auditable runtime         │
└─────────────────────────────────────────────────────────────┘

  #40 Implement cumulative budget enforcer    [critical]  60m  → needs #39
  #41 Add tool/provider circuit breakers      [high]      60m  → needs #39
  #42 Prompt injection policy actions         [high]      30m  → independent
  #43 Data classification and redaction       [high]      45m  → needs #42
  #44 Add content policy hook                 [medium]    30m  → needs #42
  #45 Add output verification                 [high]      60m  → needs #41
```

### Phase 2D — Retrieval and Telemetry Expansion

```
┌─────────────────────────────────────────────────────────────┐
│  MAX PARALLELISM: 5 tasks                                   │
│  Add retrieval backends and production observability        │
└─────────────────────────────────────────────────────────────┘

  #36 Implement sqlite-vss adapter            [high]      75m  → needs #35,#29
  #37 Add Qdrant adapter                      [medium]    60m  → needs #35
  #47 Harden OpenTelemetry export             [high]      60m  → needs #30,#41
  #48 Add Prometheus metrics endpoint         [high]      60m  → needs #47
  #34 Add retention and archival policy       [medium]    30m  → needs #29,#30
```

### Phase 2E — Operations and Escalation

```
┌─────────────────────────────────────────────────────────────┐
│  MAX PARALLELISM: 3 tasks                                   │
│  Finish operational workflows and incident readiness        │
└─────────────────────────────────────────────────────────────┘

  #46 Add heuristic escalation triggers       [medium]    45m  → needs #40,#45
  #49 Create dashboards, alerts, runbooks     [medium]    75m  → needs #47,#48
```

---

## 5. Tracking Progress

### 5.1 Database Commands

```bash
# View Phase 2 tasks (assuming numbering continues from Phase 1)
sqlite3 docs/agent_context.db "SELECT id, title, priority, status FROM items WHERE id BETWEEN 28 AND 49 ORDER BY id"

# Mark a task in progress
sqlite3 docs/agent_context.db "UPDATE items SET status = 'in_progress', updated_at = datetime('now') WHERE id = 28"

# Mark complete
sqlite3 docs/agent_context.db "UPDATE items SET status = 'complete', updated_at = datetime('now') WHERE id = 28"

# Log progress
sqlite3 docs/agent_context.db "INSERT INTO entries (session_id, entry_type, content) VALUES (2, 'progress', 'Completed persistence backend trait definitions')"
```

### 5.2 Status Values

- `pending` — Not started
- `in_progress` — Currently being worked on
- `complete` — Finished and verified
- `blocked` — Waiting on dependency
- `deferred` — Postponed to later phase

---

## 6. File Structure (Target)

```text
src/
├── core.rs
├── event.rs
├── guardrails.rs
├── lib.rs
├── memory.rs
├── phase.rs
├── tool.rs
├── validation.rs
│
├── checkpoint.rs            # NEW: checkpoint envelope + resume validation
├── persistence.rs           # NEW: persistence traits
├── persistence/
│   └── sqlite.rs            # NEW: SQLite-backed session/event/checkpoint store
├── memory/
│   ├── vector.rs            # NEW: VectorMemory trait + retrieval policy
│   ├── sqlite_vss.rs        # NEW: embedded vector backend
│   └── qdrant.rs            # NEW: optional remote vector backend
├── observability.rs         # NEW: OTEL + Prometheus wiring
│
└── tests/                   # extend with persistence, resume, guardrail, and ops coverage

migrations/                  # NEW: SQLite DDL and migration metadata
deploy/
├── grafana/                 # NEW: dashboards/provisioning
└── prometheus/              # NEW: alert rules / scrape config examples
docs/
└── runbooks/                # NEW: incident response docs
```

---

## 7. Notes

### 7.1 Embedded-First Discipline

Phase 2 should keep the **embedded SQLite path as the default production story**:
- No mandatory external database
- No mandatory vector service
- Remote stores are optional adapters behind feature flags

### 7.2 Backward Compatibility

Phase 2 should preserve the simple Phase 1 developer experience:
- Existing in-memory workflows remain valid
- Durable persistence is opt-in through config
- Guardrails default to safe behavior but remain configurable
- Observability exporters are feature/config driven rather than hard-wired

### 7.3 Testing Expectations

Phase 2 needs broader verification than Phase 1:
- Migration tests for schema upgrades and rollback safety
- Resume tests for valid and incompatible checkpoints
- Guardrail tests for reject/sanitize/audit paths
- Circuit breaker tests for closed/open/half-open transitions
- Metrics export tests for stable names and labels

---

## 8. Changelog

| Date | Change |
|------|--------|
| 2026-03-30 | Initial decomposition for Phase 2 created with tasks #28-#49 |
