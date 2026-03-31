//! Opt-in transport primitives for distributed agent execution.

use crate::agent_comm::MessageId;
use crate::error::{SwarmError, SwarmResult};
use crate::event::TraceId;
use crate::types::AgentRef;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::time::Duration;
use url::Url;

/// Location-transparent address for an agent.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentAddress {
    Local { agent: AgentRef },
    Remote { base_url: String, agent: AgentRef },
}

impl AgentAddress {
    pub fn local(agent: impl Into<AgentRef>) -> Self {
        Self::Local {
            agent: agent.into(),
        }
    }

    pub fn remote(base_url: impl Into<String>, agent: impl Into<AgentRef>) -> SwarmResult<Self> {
        let base_url = base_url.into();
        let parsed = Url::parse(&base_url).map_err(|error| {
            SwarmError::ValidationError(format!("Invalid remote agent base URL: {}", error))
        })?;
        match parsed.scheme() {
            "http" | "https" => {}
            scheme => {
                return Err(SwarmError::ValidationError(format!(
                    "Remote agent base URL must use http or https, got '{}'",
                    scheme
                )));
            }
        }
        Ok(Self::Remote {
            base_url,
            agent: agent.into(),
        })
    }

    pub fn agent_ref(&self) -> &AgentRef {
        match self {
            Self::Local { agent } | Self::Remote { agent, .. } => agent,
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local { .. })
    }

    pub fn base_url(&self) -> Option<&str> {
        match self {
            Self::Local { .. } => None,
            Self::Remote { base_url, .. } => Some(base_url),
        }
    }
}

impl fmt::Display for AgentAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local { agent } => write!(f, "local:{}", agent),
            Self::Remote { base_url, agent } => write!(f, "{}#{}", base_url, agent),
        }
    }
}

/// Network-serializable message envelope for transport backends.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DistributedMessage {
    pub id: MessageId,
    pub from: AgentAddress,
    pub to: AgentAddress,
    pub payload: Value,
    pub timestamp: DateTime<Utc>,
    pub correlation_id: Option<MessageId>,
    pub trace_id: Option<TraceId>,
    pub is_reply: bool,
}

impl DistributedMessage {
    pub fn new(from: AgentAddress, to: AgentAddress, payload: Value) -> Self {
        Self {
            id: MessageId::new(),
            from,
            to,
            payload,
            timestamp: Utc::now(),
            correlation_id: None,
            trace_id: None,
            is_reply: false,
        }
    }

    pub fn reply(original: &DistributedMessage, from: AgentAddress, payload: Value) -> Self {
        Self {
            id: MessageId::new(),
            from,
            to: original.from.clone(),
            payload,
            timestamp: Utc::now(),
            correlation_id: Some(original.id.clone()),
            trace_id: original.trace_id.clone(),
            is_reply: true,
        }
    }

    pub fn with_trace_id(mut self, trace_id: TraceId) -> Self {
        self.trace_id = Some(trace_id);
        self
    }
}

#[async_trait]
pub trait DistributedTransport: Send + Sync {
    async fn send(&self, message: DistributedMessage) -> SwarmResult<()>;
    async fn request(
        &self,
        message: DistributedMessage,
        timeout: Duration,
    ) -> SwarmResult<DistributedMessage>;
}

/// Simple HTTP transport for remote agent endpoints.
///
/// Endpoint contract:
/// - `POST /agents/{agent}/messages` accepts a [`DistributedMessage`]
/// - `POST /agents/{agent}/request?timeout_ms=...` accepts a
///   [`DistributedMessage`] and returns a [`DistributedMessage`]
#[derive(Clone)]
pub struct HttpDistributedTransport {
    client: Client,
    /// Optional `Authorization` header value (e.g. `"Bearer <token>"`).
    auth_header: Option<String>,
}

impl HttpDistributedTransport {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            auth_header: None,
        }
    }

    /// Create a transport that attaches a fixed `Authorization` header to every
    /// outbound request.  Pass the full header value, e.g. `"Bearer <token>"`.
    pub fn new_with_auth(client: Client, auth_header: String) -> Self {
        Self {
            client,
            auth_header: Some(auth_header),
        }
    }

    fn endpoint_url(address: &AgentAddress, suffix: &str) -> SwarmResult<Url> {
        let AgentAddress::Remote { base_url, agent } = address else {
            return Err(SwarmError::ValidationError(
                "HTTP distributed transport requires a remote destination".to_string(),
            ));
        };

        // Guard against path-traversal characters in the agent name.
        let name = agent.as_str();
        if name.contains('/')
            || name.contains('\\')
            || name.contains("..")
            || name.contains("%2F")
            || name.contains("%2f")
            || name.contains("%5C")
            || name.contains("%5c")
        {
            return Err(SwarmError::ValidationError(format!(
                "Agent name '{}' contains path-traversal characters",
                name
            )));
        }

        let mut url = Url::parse(base_url).map_err(|error| {
            SwarmError::ValidationError(format!("Invalid remote agent base URL: {}", error))
        })?;
        url.path_segments_mut()
            .map_err(|_| {
                SwarmError::ValidationError(
                    "Remote agent base URL cannot be used as a path base".to_string(),
                )
            })?
            .pop_if_empty()
            .extend(["agents", name, suffix]);
        Ok(url)
    }
}

#[async_trait]
impl DistributedTransport for HttpDistributedTransport {
    async fn send(&self, message: DistributedMessage) -> SwarmResult<()> {
        let url = Self::endpoint_url(&message.to, "messages")?;
        let mut req = self.client.post(url).json(&message);
        if let Some(auth) = &self.auth_header {
            req = req.header(reqwest::header::AUTHORIZATION, auth);
        }
        let response = req
            .send()
            .await
            .map_err(|error| SwarmError::NetworkError(error.to_string()))?;
        response
            .error_for_status()
            .map_err(|error| SwarmError::NetworkError(error.to_string()))?;
        Ok(())
    }

    async fn request(
        &self,
        message: DistributedMessage,
        timeout: Duration,
    ) -> SwarmResult<DistributedMessage> {
        let mut url = Self::endpoint_url(&message.to, "request")?;
        url.query_pairs_mut()
            .append_pair("timeout_ms", &timeout.as_millis().to_string());
        let mut req = self.client.post(url).timeout(timeout).json(&message);
        if let Some(auth) = &self.auth_header {
            req = req.header(reqwest::header::AUTHORIZATION, auth);
        }
        let response = req
            .send()
            .await
            .map_err(|error| SwarmError::NetworkError(error.to_string()))?;
        let response = response
            .error_for_status()
            .map_err(|error| SwarmError::NetworkError(error.to_string()))?;
        response
            .json::<DistributedMessage>()
            .await
            .map_err(|error| SwarmError::DeserializationError(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_remote_address_rejects_non_http_urls() {
        let err = AgentAddress::remote("ftp://example.com", "remote").expect_err("invalid scheme");
        assert!(err.to_string().contains("http or https"));
    }

    #[test]
    fn test_distributed_message_reply_preserves_trace() {
        let request = DistributedMessage::new(
            AgentAddress::local("alice"),
            AgentAddress::local("bob"),
            json!({"ping": true}),
        )
        .with_trace_id(TraceId::from("trace-123"));

        let reply =
            DistributedMessage::reply(&request, AgentAddress::local("bob"), json!({"ack": true}));

        assert_eq!(
            reply.trace_id.as_ref().map(TraceId::as_str),
            Some("trace-123")
        );
        assert!(reply.is_reply);
        assert_eq!(reply.to, request.from);
    }

    #[test]
    fn test_endpoint_url_rejects_path_traversal_agent_names() {
        let address = AgentAddress::remote("https://example.com", "../etc/passwd")
            .expect("remote address should parse");
        let error = HttpDistributedTransport::endpoint_url(&address, "messages")
            .expect_err("path traversal must be rejected");
        assert!(error.to_string().contains("path-traversal"));
    }

    #[tokio::test]
    async fn test_new_with_auth_sends_authorization_header() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/agents/remote/messages"))
            .and(header("authorization", "Bearer secret-token"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let transport = HttpDistributedTransport::new_with_auth(
            Client::new(),
            "Bearer secret-token".to_string(),
        );
        let remote = AgentAddress::remote(mock_server.uri(), "remote").expect("remote");
        let message =
            DistributedMessage::new(AgentAddress::local("alice"), remote, json!({"ping": true}));

        transport.send(message).await.expect("authorized send");
    }
}
