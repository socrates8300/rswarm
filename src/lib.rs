#[allow(unused_imports, unused_variables, dead_code)]
pub mod constants;
pub mod core;
pub mod types;
pub mod util;

pub use crate::core::Swarm;
pub use crate::types::{Agent, Instructions, Response};

#[allow(unused_imports, unused_variables, dead_code)]
use anyhow::Result;

#[allow(unused_imports, unused_variables, dead_code)]
use dotenv::dotenv;
#[allow(unused_imports, unused_variables, dead_code)]
use std::collections::HashMap;

#[tokio::test]
async fn test_swarm_run() -> Result<()> {
    // Load environment variables from .env file
    dotenv().ok();
    // Define a test API key (use a valid key or mock the Swarm for real tests)
    let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");
    // Define the model to be used
    let model = "gpt-4".to_string();
    // Define a test prompt
    let prompt = "This is a test prompt for the Swarm agent.".to_string();
    // Create an agent with the test prompt as instructions
    let agent = Agent {
        name: "TestAgent".to_string(),
        model: model.clone(),
        instructions: Instructions::Text(prompt),
        functions: Vec::new(),
        function_call: None,
        parallel_tool_calls: true,
    };
    // Initialize empty messages and context variables
    let messages = Vec::new();
    let context_variables = HashMap::new();
    // Create a Swarm instance with the test API key
    let swarm = Swarm::new(None, Some(api_key.to_string()), HashMap::new());
    // Run the swarm with the agent
    let response = swarm
        .run(
            agent,
            messages,
            context_variables,
            Some(model), // Model override
            false,       // Do not stream
            false,       // Debug mode off
            usize::MAX,  // Max turns
        )
        .await?;
    // Assert that the response contains messages
    assert!(
        !response.messages.is_empty(),
        "No messages returned from Swarm."
    );
    // Optionally, print the response messages for debugging
    for message in response.messages {
        println!("{}: {}", message.role, message.content.unwrap_or_default());
    }
    Ok(())
}

