#[allow(unused_imports, unused_variables, dead_code)]
pub mod constants;
pub mod core;
pub mod types;
pub mod util;
pub mod validation;

pub use crate::core::Swarm;
pub use crate::types::{Agent, Instructions, Response};

pub mod error;
pub use error::{SwarmError, SwarmResult};

#[allow(unused_imports, unused_variables, dead_code)]
use anyhow::Result;

#[allow(unused_imports, unused_variables, dead_code)]
use dotenv::dotenv;
#[allow(unused_imports, unused_variables, dead_code)]
use std::collections::HashMap;

#[tokio::test]
async fn test_swarm_run() -> Result<()> {
    dotenv().ok();

    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| SwarmError::ConfigError("OPENAI_API_KEY not set".to_string()))?;

    let model = "gpt-4".to_string();
    let prompt = "This is a test prompt for the Swarm agent.".to_string();

    let agent = Agent {
        name: "TestAgent".to_string(),
        model: model.clone(),
        instructions: Instructions::Text(prompt),
        functions: Vec::new(),
        function_call: None,
        parallel_tool_calls: true,
    };

    let messages = Vec::new();
    let context_variables = HashMap::new();

    let swarm = Swarm::new(None, Some(api_key.to_string()), HashMap::new())?;

    let response = swarm
        .run(
            agent,
            messages,
            context_variables,
            Some(model),
            false,
            false,
            usize::MAX,
        )
        .await?;

    assert!(!response.messages.is_empty(), "No messages returned from Swarm.");

    for message in response.messages {
        if let Some(content) = message.content {
            println!("{}: {}", message.role, content);
        }
    }

    Ok(())
}