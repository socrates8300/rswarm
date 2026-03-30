# Runbook: Budget Limit Exhausted

**Alert**: `RswarmBudgetExhausted`
**Severity**: Warning
**Metric**: `rswarm_budget_exhausted_total`

---

## Symptom

An agent run was terminated by the `BudgetEnforcer` because a configured
resource limit was reached before the task completed.

The `limit_type` label identifies which limit was hit:

| `limit_type` | Meaning |
|---|---|
| `token_budget` | Cumulative tokens exceeded `RuntimeLimits.token_budget` |
| `wall_time` | Elapsed seconds exceeded `RuntimeLimits.max_wall_time_secs` |
| `tool_call_quota` | Total tool calls exceeded `RuntimeLimits.max_tool_calls` |
| `max_depth` | Agent-handoff nesting exceeded `RuntimeLimits.max_depth` |

---

## Likely Causes

| Cause | Indicator |
|-------|-----------|
| Limit set too low for the task | Exhaustion on first or second run of a normal prompt |
| Agent in a runaway loop | High `rswarm_iteration_total` + escalation triggers firing |
| Unexpectedly complex prompt | Single-run exhaustion with a novel input |
| Model producing verbose output | `token_budget` hit; large `completion_tokens` in logs |
| Deeply nested agent handoffs | `max_depth` hit; review agent graph |

---

## Investigation

```bash
# 1. Which limit type is firing most often?
curl -s http://localhost:9090/api/v1/query \
  --data-urlencode 'query=sum by (limit_type)(rate(rswarm_budget_exhausted_total[1h]))' \
  | jq '.data.result'

# 2. Check iteration rate at time of exhaustion
curl -s http://localhost:9090/api/v1/query_range \
  --data-urlencode 'query=rate(rswarm_iteration_total[1m])' \
  --data-urlencode 'start=<epoch_of_alert>' \
  --data-urlencode 'end=<epoch_of_alert+300>' \
  --data-urlencode 'step=15'

# 3. Find the session in the SQLite DB
sqlite3 docs/agent_context.db \
  "SELECT session_id, agent_name, outcome, started_at, ended_at
   FROM sessions WHERE outcome LIKE '%budget%' ORDER BY started_at DESC LIMIT 10"

# 4. Inspect the checkpoint (if saved) to see the last messages
sqlite3 docs/agent_context.db \
  "SELECT payload FROM checkpoints WHERE session_id = '<SESSION_ID>'
   ORDER BY version DESC LIMIT 1" | python3 -m json.tool
```

---

## Resolution

### If the limit is too tight for legitimate workloads

Increase the relevant limit in `SwarmConfig::runtime_limits`:

```rust
let mut config = SwarmConfig::default();
config.runtime_limits = RuntimeLimits {
    token_budget:       Some(50_000),  // increase as needed
    max_wall_time_secs: Some(300),
    max_tool_calls:     Some(100),
    ..Default::default()
};
```

### If a runaway loop is the root cause

1. Check the `rswarm_guardrail_triggered_total{type="loop_detected"}` metric.
2. Review the agent's instructions — add explicit stopping conditions.
3. Lower `EscalationConfig::loop_occurrence_threshold` to catch loops earlier.
4. Consider enabling `EscalationAction::Stop` instead of `InjectWarning`.

### If a single unusually large prompt is the cause

1. Pre-truncate or summarise long inputs before passing them to the agent.
2. Use `SlidingWindowMemory` to cap the context window.

---

## Prevention

- Set conservative limits in production; loosen them only for known expensive
  tasks.
- Wire up `EscalationDetector` to catch loop patterns before the budget runs
  out.
- Monitor `rswarm_iteration_total` rate — a sudden spike is an early warning.
