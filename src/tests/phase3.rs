#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::agent_comm::AgentChannel;
    use crate::core::Swarm;
    use crate::distribution::{AgentAddress, DistributedMessage};
    use crate::event::{AgentEvent, EventSubscriber};
    use crate::team::{ConsensusStrategy, TeamFormationPolicy, TeamRole, TeamVote};
    use crate::types::{Agent, AgentRef, Instructions};

    struct CollectingSubscriber {
        events: Mutex<Vec<AgentEvent>>,
    }

    impl CollectingSubscriber {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                events: Mutex::new(Vec::new()),
            })
        }

        fn collected(&self) -> Vec<AgentEvent> {
            self.events.lock().expect("collector lock").clone()
        }
    }

    #[async_trait]
    impl EventSubscriber for CollectingSubscriber {
        async fn on_event(&self, event: &AgentEvent) {
            self.events
                .lock()
                .expect("collector lock")
                .push(event.clone());
        }
    }

    fn agent(name: &str, capabilities: &[&str]) -> Agent {
        Agent::new(
            name,
            "gpt-4",
            Instructions::Text(format!("{} instructions", name)),
        )
        .expect("agent")
        .with_capabilities(
            capabilities
                .iter()
                .map(|capability| capability.to_string())
                .collect(),
        )
    }

    #[tokio::test]
    async fn test_local_message_send_uses_swarm_runtime_and_emits_event() {
        let collector = CollectingSubscriber::new();
        let swarm = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_agent(agent("alice", &["planning"]))
            .with_agent(agent("bob", &["review"]))
            .with_subscriber(collector.clone())
            .build()
            .expect("swarm");

        let bob_channel = swarm.open_agent_channel("bob").await.expect("channel");
        let message_id = swarm
            .send_agent_message(
                None,
                AgentAddress::local("alice"),
                AgentAddress::local("bob"),
                json!({"hello": "world"}),
            )
            .await
            .expect("send message");

        let received = bob_channel.recv().await.expect("receive");
        assert_eq!(received.id, message_id);
        assert_eq!(received.payload, json!({"hello": "world"}));
        assert!(collector
            .collected()
            .iter()
            .any(|event| matches!(event, AgentEvent::MessageSent { to, .. } if *to == AgentAddress::local("bob"))));
    }

    #[tokio::test]
    async fn test_local_request_reply_emits_receive_event() {
        let collector = CollectingSubscriber::new();
        let swarm = Arc::new(
            Swarm::builder()
                .with_api_key("sk-test".to_string())
                .with_agent(agent("alice", &["planning"]))
                .with_agent(agent("bob", &["review"]))
                .with_subscriber(collector.clone())
                .build()
                .expect("swarm"),
        );

        let bob_channel = swarm.open_agent_channel("bob").await.expect("channel");
        tokio::spawn(async move {
            let incoming = bob_channel.recv().await.expect("incoming");
            let reply = crate::agent_comm::AgentMessage::reply(
                &incoming,
                AgentRef::new("bob"),
                json!({"ack": true}),
            );
            bob_channel.send(reply).await.expect("reply send");
        });

        let reply = swarm
            .request_agent_message(
                None,
                AgentAddress::local("alice"),
                AgentAddress::local("bob"),
                json!({"ping": true}),
                Duration::from_secs(1),
            )
            .await
            .expect("reply");

        assert_eq!(reply.payload, json!({"ack": true}));
        assert!(collector
            .collected()
            .iter()
            .any(|event| matches!(event, AgentEvent::MessageReceived { by, .. } if *by == AgentAddress::local("alice"))));
    }

    #[tokio::test]
    async fn test_remote_request_uses_http_transport() {
        let mock_server = MockServer::start().await;
        let remote_address = AgentAddress::remote(mock_server.uri(), "remote").expect("remote");
        let response_message = DistributedMessage::new(
            remote_address.clone(),
            AgentAddress::local("alice"),
            json!({"answer": 42}),
        );

        Mock::given(method("POST"))
            .and(path("/agents/remote/request"))
            .and(query_param("timeout_ms", "250"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response_message))
            .mount(&mock_server)
            .await;

        let collector = CollectingSubscriber::new();
        let swarm = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_agent(agent("alice", &["planning"]))
            .with_subscriber(collector.clone())
            .build()
            .expect("swarm");

        let reply = swarm
            .request_agent_message(
                None,
                AgentAddress::local("alice"),
                remote_address,
                json!({"question": "life"}),
                Duration::from_millis(250),
            )
            .await
            .expect("remote reply");

        assert_eq!(reply.payload, json!({"answer": 42}));
        assert!(collector
            .collected()
            .iter()
            .any(|event| matches!(event, AgentEvent::MessageSent { to, .. } if matches!(to, AgentAddress::Remote { .. }))));
    }

    #[tokio::test]
    async fn test_form_team_load_balances_across_equivalent_agents() {
        let collector = CollectingSubscriber::new();
        let swarm = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_agent(agent("planner-a", &["planning"]))
            .with_agent(agent("planner-b", &["planning"]))
            .with_subscriber(collector.clone())
            .build()
            .expect("swarm");

        let role = TeamRole::new("planner", vec!["planning".to_string()]).expect("role");
        let first_team = swarm
            .form_team(std::slice::from_ref(&role))
            .await
            .expect("team");
        let second_team = swarm.form_team(&[role]).await.expect("team");

        assert_ne!(
            first_team.agent_for_role("planner"),
            second_team.agent_for_role("planner")
        );
        assert!(collector
            .collected()
            .iter()
            .any(|event| matches!(event, AgentEvent::TeamFormed { .. })));
    }

    #[tokio::test]
    async fn test_reconfigure_team_preserves_existing_assignments_when_requested() {
        let swarm = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_agent(agent("planner", &["planning"]))
            .with_agent(agent("reviewer", &["review"]))
            .with_agent(agent("researcher", &["research"]))
            .build()
            .expect("swarm");

        let planner_role = TeamRole::new("planner", vec!["planning".to_string()]).expect("role");
        let reviewer_role = TeamRole::new("reviewer", vec!["review".to_string()]).expect("role");
        let initial_team = swarm
            .form_team(&[planner_role.clone(), reviewer_role.clone()])
            .await
            .expect("initial team");

        let researcher_role =
            TeamRole::new("researcher", vec!["research".to_string()]).expect("role");
        let reconfigured = swarm
            .reconfigure_team(
                &initial_team,
                &[planner_role, reviewer_role, researcher_role],
                TeamFormationPolicy {
                    allow_agent_reuse: false,
                    prefer_existing_assignments: true,
                },
            )
            .await
            .expect("reconfigured");

        assert_eq!(
            initial_team.agent_for_role("planner"),
            reconfigured.agent_for_role("planner")
        );
        assert_eq!(
            initial_team.agent_for_role("reviewer"),
            reconfigured.agent_for_role("reviewer")
        );
    }

    #[tokio::test]
    async fn test_reach_consensus_emits_event() {
        let collector = CollectingSubscriber::new();
        let swarm = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_agent(agent("alice", &["planning"]))
            .with_agent(agent("bob", &["planning"]))
            .with_agent(agent("carol", &["planning"]))
            .with_subscriber(collector.clone())
            .build()
            .expect("swarm");

        let votes = vec![
            TeamVote::new(AgentRef::new("alice"), "approve").expect("vote"),
            TeamVote::new(AgentRef::new("bob"), "approve").expect("vote"),
            TeamVote::new(AgentRef::new("carol"), "reject").expect("vote"),
        ];

        let decision = swarm
            .reach_consensus(&votes, ConsensusStrategy::Majority)
            .await
            .expect("consensus");

        assert_eq!(decision.selected_option(), "approve");
        assert!(!decision.unanimous());
        assert!(collector
            .collected()
            .iter()
            .any(|event| matches!(event, AgentEvent::ConsensusReached { .. })));
    }
}
