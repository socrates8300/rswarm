# Runbook: Circuit Breaker Open

**Alert**: `RswarmCircuitBreakerOpen`
**Severity**: Critical
**Metric**: `rswarm_circuit_breaker_state_total{state="open"}`

---

## Symptom

A tool or LLM provider breaker has tripped to the **OPEN** state. Calls to
that component are being rejected immediately without hitting the downstream
service. Agent runs that depend on the component will fail fast.

---

## Likely Causes

| Cause | Indicator |
|-------|-----------|
| Downstream service outage | High tool error rate + breaker label matches the failing tool |
| Transient network partition | Error rate spike followed by recovery |
| Misconfigured tool (bad URL, wrong auth) | Consistent 4xx/5xx errors from tool |
| LLM provider quota exceeded | HTTP 429 or 503 from provider |
| Bug in tool implementation | Panics or logic errors causing repeated failures |

---

## Investigation

```bash
# 1. Identify which breaker(s) are open
curl -s http://localhost:9090/api/v1/query \
  --data-urlencode 'query=increase(rswarm_circuit_breaker_state_total{state="open"}[10m])' \
  | jq '.data.result[] | {breaker: .metric.breaker, value: .value[1]}'

# 2. Check the tool error rate for that breaker
curl -s http://localhost:9090/api/v1/query \
  --data-urlencode 'query=rate(rswarm_tool_outcome_total{outcome="err"}[5m])' \
  | jq '.data.result[] | select(.metric.tool == "<BREAKER_NAME>")'

# 3. Check application logs for the root error
grep 'circuit breaker' /var/log/rswarm/app.log | tail -50

# 4. Confirm if the downstream service is healthy
curl -I https://<downstream-url>/health
```

---

## Resolution

### If the downstream is recovering on its own

Wait for the reset window to elapse (configured in `CircuitBreaker::new`). The
breaker will transition to **HalfOpen** automatically. One successful probe
will close it.

### If the downstream is permanently degraded

1. Disable the affected tool in the agent configuration.
2. Optionally deploy an alternative tool implementation pointing to a healthy
   endpoint and restart the service.
3. Set `failure_threshold` higher if the breaker is too sensitive.

### If it was a transient blip and the service is healthy

```bash
# Force close the breaker by restarting the process
# (breakers reset to Closed on process start)
systemctl restart rswarm
```

### If the tool has a bug

1. Identify the root cause from logs.
2. Fix the tool implementation.
3. Redeploy.

---

## Prevention

- Set `reset_secs` to a value that matches your downstream's typical recovery
  time (30–60 s for APIs, longer for databases).
- Monitor `rswarm_tool_outcome_total` for early warning of rising error rates
  before the breaker trips.
- Consider adding a `RswarmHighToolErrorRate` alert at 25 % to alert before
  the breaker opens.
