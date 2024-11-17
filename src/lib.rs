// ./src/lib.rs
#[allow(unused_imports, unused_variables, dead_code)]
pub mod constants;
pub mod core;
pub mod types;
pub mod util;
pub mod validation;

pub use crate::core::Swarm;
pub use crate::types::{Agent, Instructions, Message, Response, SwarmConfig};

pub mod error;
pub use error::{SwarmError, SwarmResult};

#[allow(unused_imports, unused_variables, dead_code)]
use anyhow::Result;

#[allow(unused_imports, unused_variables, dead_code)]
use dotenv::dotenv;
#[allow(unused_imports, unused_variables, dead_code)]
use std::collections::HashMap;

#[tokio::test]
async fn test_swarm_builder() -> Result<()> {
    dotenv().ok();

    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| SwarmError::ConfigError("OPENAI_API_KEY not set".to_string()))?;

    let mut config = SwarmConfig::default();
    config.request_timeout = 60;
    config.connect_timeout = 15;
    config.max_retries = 5;
    config.valid_model_prefixes = vec!["gpt-".to_string(), "claude-".to_string()];

    let agent = Agent {
        name: "TestAgent".to_string(),
        model: "gpt-4".to_string(),
        instructions: Instructions::Text("This is a test prompt for the Swarm agent.".to_string()),
        functions: Vec::new(),
        function_call: None,
        parallel_tool_calls: true,
    };

    let swarm = Swarm::builder()
        .with_api_key(api_key)
        .with_agent(agent.clone())
        .with_config(config)
        .build()?;

    // Create an initial message
    let messages = vec![Message {
        role: "user".to_string(),
        content: Some("Hello, this is a test message.".to_string()),
        name: None,
        function_call: None,
    }];

    let context_variables = HashMap::new();

    let response = swarm
        .run(
            agent,
            messages,
            context_variables,
            Some("gpt-4".to_string()),
            false,
            false,
            10,
        )
        .await?;

    assert!(!response.messages.is_empty(), "No messages returned from Swarm.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swarm_builder() {
        let result = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_max_loop_iterations(100)
            .build();

        assert!(result.is_ok());

        let swarm = result.unwrap();
        assert_eq!(swarm.api_key, "sk-test");
        assert_eq!(swarm.config.max_loop_iterations, 100);
    }
}