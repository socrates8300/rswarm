//! Agent-to-agent communication primitives.
//!
//! Provides typed message passing between agents within a single process via
//! [`InProcessChannel`], which uses Tokio broadcast channels for delivery and
//! oneshot channels for request-reply correlation.

use crate::error::{SwarmError, SwarmResult};
use crate::types::AgentRef;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, oneshot, Mutex};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// MessageId
// ---------------------------------------------------------------------------

/// Opaque unique identifier for a single [`AgentMessage`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(Uuid);

impl MessageId {
    pub fn new() -> Self {
        MessageId(Uuid::new_v4())
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// AgentMessage
// ---------------------------------------------------------------------------

/// A typed message envelope for agent-to-agent communication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: MessageId,
    pub from: AgentRef,
    pub to: AgentRef,
    pub payload: Value,
    pub timestamp: DateTime<Utc>,
    /// Present on request messages; echoed by the replier so the requester
    /// can correlate the response via the pending-reply registry.
    pub correlation_id: Option<MessageId>,
    /// `true` when this message is a reply to a prior [`AgentChannel::request`] call.
    pub is_reply: bool,
}

impl AgentMessage {
    /// Create a new outbound message with a fresh [`MessageId`].
    pub fn new(from: AgentRef, to: AgentRef, payload: Value) -> Self {
        AgentMessage {
            id: MessageId::new(),
            from,
            to,
            payload,
            timestamp: Utc::now(),
            correlation_id: None,
            is_reply: false,
        }
    }

    /// Build a reply to `original`. Sets `is_reply = true` and echoes
    /// `original.id` as `correlation_id` so the requester can correlate it.
    pub fn reply(original: &AgentMessage, from: AgentRef, payload: Value) -> Self {
        AgentMessage {
            id: MessageId::new(),
            from,
            to: original.from.clone(),
            payload,
            timestamp: Utc::now(),
            correlation_id: Some(original.id.clone()),
            is_reply: true,
        }
    }
}

// ---------------------------------------------------------------------------
// AgentChannel trait
// ---------------------------------------------------------------------------

/// Typed async channel for agent-to-agent communication.
#[async_trait]
pub trait AgentChannel: Send + Sync {
    /// Send a message to the agent identified by `msg.to`.
    async fn send(&self, msg: AgentMessage) -> SwarmResult<()>;

    /// Receive the next message addressed to this agent.
    async fn recv(&self) -> SwarmResult<AgentMessage>;

    /// Send `msg` and await a correlated reply within `timeout`.
    ///
    /// Returns [`SwarmError::TimeoutError`] if no reply arrives in time.
    async fn request(&self, msg: AgentMessage, timeout: Duration) -> SwarmResult<AgentMessage>;

    /// The [`AgentRef`] this channel belongs to.
    fn agent_ref(&self) -> &AgentRef;
}

// ---------------------------------------------------------------------------
// ChannelRegistry
// ---------------------------------------------------------------------------

struct RegistryInner {
    senders: HashMap<AgentRef, broadcast::Sender<AgentMessage>>,
    pending_replies: HashMap<MessageId, oneshot::Sender<AgentMessage>>,
}

/// Shared registry for all in-process agent channels.
///
/// Holds broadcast senders (for normal messages) and oneshot senders (for
/// request-reply correlation). Create once with [`ChannelRegistry::new`] and
/// share via `Arc`.
pub struct ChannelRegistry {
    inner: Mutex<RegistryInner>,
}

impl ChannelRegistry {
    const BROADCAST_CAPACITY: usize = 64;

    pub fn new() -> Arc<Self> {
        Arc::new(ChannelRegistry {
            inner: Mutex::new(RegistryInner {
                senders: HashMap::new(),
                pending_replies: HashMap::new(),
            }),
        })
    }

    /// Register a new agent and return its broadcast receiver.
    ///
    /// Registration is exclusive while a live receiver exists for `agent`.
    /// Once all receivers are dropped, the same [`AgentRef`] may be registered
    /// again to establish a fresh mailbox.
    pub async fn register(
        &self,
        agent: &AgentRef,
    ) -> SwarmResult<broadcast::Receiver<AgentMessage>> {
        let mut guard = self.inner.lock().await;
        if let Some(existing) = guard.senders.get(agent) {
            if existing.receiver_count() > 0 {
                return Err(SwarmError::Other(format!(
                    "Agent '{}' is already registered",
                    agent
                )));
            }
        }
        let (tx, rx) = broadcast::channel(Self::BROADCAST_CAPACITY);
        guard.senders.insert(agent.clone(), tx);
        Ok(rx)
    }

    /// Route a message to its destination.
    ///
    /// If `msg.is_reply`, the message is delivered through the pending-reply
    /// oneshot keyed by `msg.correlation_id`. Otherwise it is broadcast to the
    /// registered channel for `msg.to`.
    pub async fn send(&self, msg: AgentMessage) -> SwarmResult<()> {
        tracing::debug!(
            from = %msg.from,
            to = %msg.to,
            message_id = %msg.id,
            is_reply = msg.is_reply,
            "routing agent message",
        );
        let mut guard = self.inner.lock().await;
        if msg.is_reply {
            let cid = msg.correlation_id.as_ref().ok_or_else(|| {
                SwarmError::Other("Reply message missing correlation_id".to_string())
            })?;
            let tx = guard.pending_replies.remove(cid).ok_or_else(|| {
                SwarmError::Other(format!(
                    "No pending request found for correlation id '{}'",
                    cid
                ))
            })?;
            tx.send(msg).map_err(|_| {
                SwarmError::Other("Reply receiver dropped before delivery".to_string())
            })
        } else {
            let to = msg.to.clone();
            match guard.senders.get(&to) {
                Some(tx) => tx.send(msg).map(|_| ()).map_err(|_| {
                    SwarmError::Other(format!("No active receivers for agent '{}'", to))
                }),
                None => Err(SwarmError::AgentNotFoundError(format!(
                    "No channel registered for agent '{}'",
                    to
                ))),
            }
        }
    }

    /// Send `msg` and await a correlated reply within `timeout`.
    ///
    /// This is the registry-level equivalent of [`AgentChannel::request`] and
    /// is useful for higher-level runtimes that want request-reply semantics
    /// without managing a dedicated [`InProcessChannel`] handle for the caller.
    pub async fn request(
        &self,
        mut msg: AgentMessage,
        timeout: Duration,
    ) -> SwarmResult<AgentMessage> {
        let correlation_id = msg.id.clone();
        msg.correlation_id = Some(correlation_id.clone());

        let reply_rx = self.register_pending(correlation_id.clone()).await;
        self.send(msg).await?;

        match tokio::time::timeout(timeout, reply_rx).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(_)) => Err(SwarmError::Other(
                "Reply oneshot closed unexpectedly".to_string(),
            )),
            Err(_elapsed) => {
                self.cancel_pending(&correlation_id).await;
                Err(SwarmError::TimeoutError(format!(
                    "request timed out after {}ms",
                    timeout.as_millis()
                )))
            }
        }
    }

    /// Send one message to each distinct agent in `recipients`.
    ///
    /// Recipients are validated up front to avoid partial delivery when a
    /// target is missing or has no live receivers.
    pub async fn multicast(
        &self,
        from: AgentRef,
        recipients: impl IntoIterator<Item = AgentRef>,
        payload: Value,
    ) -> SwarmResult<Vec<MessageId>> {
        let mut seen = HashSet::new();
        let recipients: Vec<AgentRef> = recipients
            .into_iter()
            .filter(|recipient| seen.insert(recipient.clone()))
            .collect();

        let senders = {
            let guard = self.inner.lock().await;
            let mut senders = Vec::with_capacity(recipients.len());
            for recipient in &recipients {
                match guard.senders.get(recipient) {
                    Some(tx) if tx.receiver_count() > 0 => {
                        senders.push((recipient.clone(), tx.clone()));
                    }
                    Some(_) => {
                        return Err(SwarmError::Other(format!(
                            "No active receivers for agent '{}'",
                            recipient
                        )));
                    }
                    None => {
                        return Err(SwarmError::AgentNotFoundError(format!(
                            "No channel registered for agent '{}'",
                            recipient
                        )));
                    }
                }
            }
            senders
        };

        let mut message_ids = Vec::with_capacity(senders.len());
        for (recipient, tx) in senders {
            let msg = AgentMessage::new(from.clone(), recipient, payload.clone());
            let message_id = msg.id.clone();
            tx.send(msg).map_err(|_| {
                SwarmError::Other("A multicast recipient lost its active receiver".to_string())
            })?;
            message_ids.push(message_id);
        }

        Ok(message_ids)
    }

    /// Broadcast to every currently registered agent.
    pub async fn broadcast(
        &self,
        from: AgentRef,
        payload: Value,
        include_sender: bool,
    ) -> SwarmResult<Vec<MessageId>> {
        let recipients = {
            let guard = self.inner.lock().await;
            guard
                .senders
                .keys()
                .filter(|recipient| include_sender || **recipient != from)
                .cloned()
                .collect::<Vec<_>>()
        };
        self.multicast(from, recipients, payload).await
    }

    /// Register a pending-reply slot and return the receiver half.
    ///
    /// Call this *before* sending the request to avoid a race where the reply
    /// arrives before the slot is registered.
    pub async fn register_pending(&self, id: MessageId) -> oneshot::Receiver<AgentMessage> {
        let (tx, rx) = oneshot::channel();
        self.inner.lock().await.pending_replies.insert(id, tx);
        rx
    }

    /// Remove an orphaned pending-reply slot (e.g. after a timeout).
    pub async fn cancel_pending(&self, id: &MessageId) {
        self.inner.lock().await.pending_replies.remove(id);
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        ChannelRegistry {
            inner: Mutex::new(RegistryInner {
                senders: HashMap::new(),
                pending_replies: HashMap::new(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// InProcessChannel
// ---------------------------------------------------------------------------

/// In-process implementation of [`AgentChannel`] backed by Tokio broadcast
/// and oneshot channels.
pub struct InProcessChannel {
    agent_ref: AgentRef,
    registry: Arc<ChannelRegistry>,
    /// Wrapped in `Mutex` so `recv()` can take `&self`.
    receiver: Mutex<broadcast::Receiver<AgentMessage>>,
}

impl InProcessChannel {
    pub async fn new(agent_ref: AgentRef, registry: Arc<ChannelRegistry>) -> SwarmResult<Self> {
        let receiver = registry.register(&agent_ref).await?;
        Ok(InProcessChannel {
            agent_ref,
            registry,
            receiver: Mutex::new(receiver),
        })
    }
}

#[async_trait]
impl AgentChannel for InProcessChannel {
    async fn send(&self, msg: AgentMessage) -> SwarmResult<()> {
        self.registry.send(msg).await
    }

    async fn recv(&self) -> SwarmResult<AgentMessage> {
        let mut guard = self.receiver.lock().await;
        loop {
            match guard.recv().await {
                Ok(msg) => {
                    tracing::debug!(
                        by = %self.agent_ref,
                        from = %msg.from,
                        message_id = %msg.id,
                        is_reply = msg.is_reply,
                        "received agent message",
                    );
                    return Ok(msg);
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(
                        agent = %self.agent_ref,
                        "Agent channel lagged, {} messages dropped",
                        n
                    );
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(SwarmError::Other("Agent channel closed".to_string()));
                }
            }
        }
    }

    async fn request(&self, msg: AgentMessage, timeout: Duration) -> SwarmResult<AgentMessage> {
        let destination = msg.to.clone();
        let correlation_id = msg.id.clone();
        match self.registry.request(msg, timeout).await {
            Ok(reply) => Ok(reply),
            Err(err) => {
                if matches!(err, SwarmError::TimeoutError(_)) {
                    tracing::warn!(
                        from = %self.agent_ref,
                        to = %destination,
                        correlation_id = %correlation_id,
                        timeout_ms = timeout.as_millis() as u64,
                        "agent request timed out",
                    );
                }
                Err(err)
            }
        }
    }

    fn agent_ref(&self) -> &AgentRef {
        &self.agent_ref
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn make_channel(registry: Arc<ChannelRegistry>, name: &str) -> InProcessChannel {
        InProcessChannel::new(AgentRef::new(name), registry)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_send_recv() {
        let reg = ChannelRegistry::new();
        let alice = make_channel(reg.clone(), "alice").await;
        let bob = make_channel(reg.clone(), "bob").await;

        let msg = AgentMessage::new(
            AgentRef::new("alice"),
            AgentRef::new("bob"),
            json!({"hello": "world"}),
        );
        alice.send(msg.clone()).await.unwrap();

        let received = bob.recv().await.unwrap();
        assert_eq!(received.id, msg.id);
        assert_eq!(received.payload, json!({"hello": "world"}));
    }

    #[tokio::test]
    async fn test_request_reply() {
        let reg = ChannelRegistry::new();
        let alice = make_channel(reg.clone(), "alice").await;
        let bob = make_channel(reg.clone(), "bob").await;

        let reg2 = reg.clone();
        tokio::spawn(async move {
            let incoming = bob.recv().await.unwrap();
            let reply = AgentMessage::reply(&incoming, AgentRef::new("bob"), json!({"ack": true}));
            reg2.send(reply).await.unwrap();
        });

        let request = AgentMessage::new(
            AgentRef::new("alice"),
            AgentRef::new("bob"),
            json!({"ping": 1}),
        );
        let reply = alice
            .request(request, Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(reply.payload, json!({"ack": true}));
        assert!(reply.is_reply);
        assert!(reply.correlation_id.is_some());
    }

    #[tokio::test]
    async fn test_request_timeout() {
        let reg = ChannelRegistry::new();
        let alice = make_channel(reg.clone(), "alice").await;
        let _bob = make_channel(reg.clone(), "bob").await; // registered but never replies

        let request = AgentMessage::new(
            AgentRef::new("alice"),
            AgentRef::new("bob"),
            json!({"ping": 1}),
        );
        let result = alice.request(request, Duration::from_millis(50)).await;
        assert!(matches!(result, Err(SwarmError::TimeoutError(ref s)) if s.contains("timed out")));
    }

    #[tokio::test]
    async fn test_send_to_unregistered_agent_errors() {
        let reg = ChannelRegistry::new();
        let alice = make_channel(reg.clone(), "alice").await;

        let msg = AgentMessage::new(AgentRef::new("alice"), AgentRef::new("nobody"), json!(null));
        let result = alice.send(msg).await;
        assert!(matches!(result, Err(SwarmError::AgentNotFoundError(_))));
    }

    #[tokio::test]
    async fn test_duplicate_registration_errors_while_original_is_alive() {
        let reg = ChannelRegistry::new();
        let _alice = make_channel(reg.clone(), "alice").await;

        let duplicate = InProcessChannel::new(AgentRef::new("alice"), reg.clone()).await;
        assert!(
            matches!(duplicate, Err(SwarmError::Other(ref s)) if s.contains("already registered"))
        );
    }

    #[tokio::test]
    async fn test_reregister_after_drop_is_allowed() {
        let reg = ChannelRegistry::new();
        {
            let _alice = make_channel(reg.clone(), "alice").await;
        }

        let alice = InProcessChannel::new(AgentRef::new("alice"), reg.clone())
            .await
            .expect("dropped receiver should allow fresh registration");
        let bob = make_channel(reg.clone(), "bob").await;
        bob.send(AgentMessage::new(
            AgentRef::new("bob"),
            AgentRef::new("alice"),
            json!({"hello": "again"}),
        ))
        .await
        .unwrap();

        let received = alice.recv().await.unwrap();
        assert_eq!(received.payload, json!({"hello": "again"}));
    }

    #[tokio::test]
    async fn test_reply_without_pending_slot_errors() {
        let reg = ChannelRegistry::new();
        let alice = make_channel(reg.clone(), "alice").await;
        let reply = AgentMessage {
            id: MessageId::new(),
            from: AgentRef::new("alice"),
            to: AgentRef::new("bob"),
            payload: json!({"ack": true}),
            timestamp: Utc::now(),
            correlation_id: Some(MessageId::new()),
            is_reply: true,
        };

        let result = alice.send(reply).await;
        assert!(
            matches!(result, Err(SwarmError::Other(ref s)) if s.contains("No pending request"))
        );
    }

    #[tokio::test]
    async fn test_multicast_delivers_to_requested_agents() {
        let reg = ChannelRegistry::new();
        let _alice = make_channel(reg.clone(), "alice").await;
        let bob = make_channel(reg.clone(), "bob").await;
        let carol = make_channel(reg.clone(), "carol").await;

        let ids = reg
            .multicast(
                AgentRef::new("alice"),
                vec![AgentRef::new("bob"), AgentRef::new("carol")],
                json!({"topic": "update"}),
            )
            .await
            .unwrap();

        assert_eq!(ids.len(), 2);
        assert_eq!(
            bob.recv().await.unwrap().payload,
            json!({"topic": "update"})
        );
        assert_eq!(
            carol.recv().await.unwrap().payload,
            json!({"topic": "update"})
        );
    }

    #[tokio::test]
    async fn test_broadcast_skips_sender_when_requested() {
        let reg = ChannelRegistry::new();
        let alice = make_channel(reg.clone(), "alice").await;
        let bob = make_channel(reg.clone(), "bob").await;
        let carol = make_channel(reg.clone(), "carol").await;

        let ids = reg
            .broadcast(AgentRef::new("alice"), json!({"kind": "broadcast"}), false)
            .await
            .unwrap();

        assert_eq!(ids.len(), 2);
        assert_eq!(
            bob.recv().await.unwrap().payload,
            json!({"kind": "broadcast"})
        );
        assert_eq!(
            carol.recv().await.unwrap().payload,
            json!({"kind": "broadcast"})
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(25), alice.recv())
                .await
                .is_err()
        );
    }
}
