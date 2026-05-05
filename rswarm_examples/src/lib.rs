mod browse_docs;

pub use crate::browse_docs::browse_rust_docs;
pub use rswarm::core::Swarm;
pub use rswarm::types::{Agent, Instructions, Response};

#[cfg(test)]
mod tests {
    use rswarm::{Agent, Instructions, Message, RunOptions, Swarm};
    use std::path::Path;

    /// Ensure Swarm::run rejects an empty message history — guards against
    /// EXAMPLES-001 regressing (passing Vec::new() to run()).
    #[tokio::test]
    async fn swarm_run_rejects_empty_messages() {
        let agent =
            Agent::new("test", "gpt-4o", Instructions::Text("You help.".into())).expect("agent");
        let swarm = Swarm::builder()
            .with_api_key("sk-test-key-for-validation-only".to_string())
            .with_agent(agent.clone())
            .build()
            .expect("swarm");

        let result = swarm
            .run(agent, vec![], RunOptions::new().with_max_turns(1))
            .await;

        assert!(
            result.is_err(),
            "Swarm::run must fail when messages is empty"
        );
    }

    /// Confirm that a seeded initial message does not fail validation.
    #[test]
    fn initial_message_is_valid() {
        let msg = Message::user("Hello! Please help me browse Rust documentation.");
        assert!(
            msg.is_ok(),
            "Message::user must succeed with non-empty content"
        );
    }

    #[test]
    fn prompt_file_is_available_from_manifest_dir() {
        let prompt_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("prompt.txt");
        assert!(
            prompt_path.exists(),
            "expected example prompt at {}",
            prompt_path.display()
        );
    }
}
