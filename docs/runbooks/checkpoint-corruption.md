# Runbook: Checkpoint Corruption / Resume Failure

**Alert**: (no dedicated alert; triggered by `SwarmError` on resume)
**Severity**: Warning / Operational
**Error variants**:
- `CheckpointEnvelope::validate()` → version mismatch or empty session_id
- `Swarm::resume_from_checkpoint()` → agent not found, no checkpoint, max iterations exhausted

---

## Symptom

A call to `Swarm::resume_from_checkpoint()` returns an error, preventing a
session from being resumed. This can manifest as:

- A `SwarmError::Other("Checkpoint version X is incompatible…")` log line.
- A `SwarmError::AgentNotFoundError` referencing an agent from the checkpoint.
- A `SwarmError::MaxIterationsError` indicating the checkpoint was at the last
  allowed iteration.

---

## Likely Causes

| Cause | Indicator |
|-------|-----------|
| Checkpoint written by an older binary | `version` field < `CURRENT_CHECKPOINT_VERSION` |
| Agent renamed or removed after checkpoint | `AgentNotFoundError` with the old agent name |
| Session reached `max_loop_iterations` | `MaxIterationsError` on resume |
| Corrupted JSON in checkpoint payload | `DeserializationError` from `CheckpointEnvelope::from_json` |
| SQLite file deleted or moved | `SqliteStore::open` fails; no checkpoints found |

---

## Investigation

```bash
# 1. List checkpoints for the affected session
sqlite3 <db_path> \
  "SELECT session_id, version, created_at FROM checkpoints
   WHERE session_id = '<SESSION_ID>' ORDER BY version DESC"

# 2. Inspect the envelope
sqlite3 <db_path> \
  "SELECT payload FROM checkpoints WHERE session_id = '<SESSION_ID>'
   ORDER BY version DESC LIMIT 1" | python3 -c "
import sys, json
data = json.load(sys.stdin)
print('version  :', data.get('version'))
print('agent    :', data.get('payload', {}).get('current_agent'))
print('iteration:', data.get('payload', {}).get('iteration'))
print('messages :', len(data.get('payload', {}).get('messages', [])))
"

# 3. Confirm the current checkpoint version in code
grep 'CURRENT_CHECKPOINT_VERSION' src/checkpoint.rs
```

---

## Resolution

### Version mismatch (old checkpoint, new binary)

If the checkpoint data is still logically valid, write a one-off migration:

```rust
// Load the raw JSON
let json = store.load_checkpoint("session-id").await?
    .expect("checkpoint exists");

// Manually patch the version field
let mut raw: serde_json::Value = serde_json::from_str(&json.to_json()?)?;
raw["version"] = serde_json::json!(CURRENT_CHECKPOINT_VERSION);

// Re-save
let migrated = CheckpointEnvelope::from_json(&raw.to_string())?;
store.save_checkpoint(&migrated).await?;
```

For systematic schema changes, add a `v001_to_v002.rs` migration and bump
`CURRENT_CHECKPOINT_VERSION`.

### Agent not found

The checkpoint references an agent that no longer exists in the registry.

1. Re-register the old agent under the same name (even as a stub) to allow
   resume, then let the new logic take over.
2. Or start a fresh session from the last known messages:

```rust
let envelope = store.load_checkpoint("session-id").await?.unwrap();
let new_agent = swarm.get_agent_by_name("replacement-agent")?;
swarm.run(
    new_agent,
    envelope.payload.messages,
    envelope.payload.context_variables,
    None, false, false,
    remaining_turns,
).await?;
```

### Max iterations exhausted

The checkpoint was taken at `max_loop_iterations`. The session cannot resume
within the current limit. Either:

- Increase `max_loop_iterations` in `SwarmConfig`.
- Acknowledge the session as complete (the final messages are in the checkpoint).

### Corrupted JSON

The checkpoint payload cannot be deserialized. This is unusual and indicates
filesystem or write-path corruption.

1. Attempt to restore from a previous checkpoint version:

```bash
sqlite3 <db_path> \
  "SELECT version, payload FROM checkpoints WHERE session_id = '<SESSION_ID>'
   ORDER BY version DESC"
```

2. If all versions are corrupt, the session cannot be resumed. Start a new
   session from the most recent user messages if available.

---

## Prevention

- Verify checkpoint health regularly:

```bash
sqlite3 <db_path> \
  "SELECT session_id, MAX(version) AS latest_version, COUNT(*) AS cp_count
   FROM checkpoints GROUP BY session_id ORDER BY latest_version DESC LIMIT 20"
```

- Run `RetentionPolicy::prune()` on a schedule to avoid unbounded growth that
  can slow checkpoint writes.
- Monitor checkpoint save failures in logs (`WARN checkpoint save failed`).
