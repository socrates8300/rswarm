// src/main.rs
mod browse_docs;

use rswarm::{Agent, Instructions, Swarm};
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use dotenv::dotenv;
use std::env;
use crate::browse_docs::browse_rust_docs;
use rswarm::types::AgentFunction;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file
    dotenv().ok();

    let browse_docs_function = AgentFunction {
        name: "browse_docs".to_string(),
        function: Arc::new(browse_rust_docs),
        accepts_context_variables: false,
    };

    // Retrieve the API key from the environment
    let api_key = env::var("OPENAI_API_KEY")
        .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY environment variable not set"))?;

    // Retrieve the model from the environment or default to "gpt-4"
    let model = env::var("OPENAI_MODEL")
        .unwrap_or_else(|_| "gpt-4".to_string());

    // Specify the path to the prompt file
    let prompt_file_path = "prompt.txt";

    // Read the prompt from the file
    let prompt = fs::read_to_string(prompt_file_path)
        .map_err(|e| anyhow::anyhow!(
            "Failed to read prompt file '{}': {}",
            prompt_file_path, e
        ))?;

    // Create your agents
    let agent_name = "Agent";
    let agent = Agent {
        name: agent_name.to_string(),
        model: model.clone(),
        instructions: Instructions::Text(prompt),
        functions: Vec::new(),
        function_call: None,
        parallel_tool_calls: true,
    };

    // Create any additional agents if needed
    // For example, AgentB
    let agent_b_name = "AgentManager";
    let agent_b_instructions = "You are a CTO with 20 years of experience, your job is to oversee the execution of tasks by other agents.  To do reviews and provide feedback.";
    let agent_b = Agent {
        name: agent_b_name.to_string(),
        model: model.clone(),
        instructions: Instructions::Text(agent_b_instructions.to_string()),
        functions: Vec::new(),
        function_call: None,
        parallel_tool_calls: true,
    };

    let docs_agent = Agent {
        name: "DocBrowserAgent".to_string(),
        model: "gpt-4".to_string(),
        instructions: Instructions::Text(
            "You can browse Rust documentation using the 'browse_docs' function. browse_docs_function takes a string as an argument (query) and returns a string.".to_string(),
        ),
        functions: vec![browse_docs_function],
        function_call: None,
        parallel_tool_calls: false,
    };

    // Create an agent registry
    let mut agents = HashMap::new();
    agents.insert(agent.name.clone(), agent.clone());   // Clone agent here
    agents.insert(agent_b.name.clone(), agent_b.clone()); // Clone agent_b here
    agents.insert(docs_agent.name.clone(), docs_agent.clone()); // Clone docs_agent here

    // Create a Swarm instance with the API key and agents
    let swarm = Swarm::builder()
        .with_api_key(api_key)
        .with_agent(agent.clone())
        .with_agent(agent_b.clone())
        .with_agent(docs_agent.clone())
        .build()?;

    // Initialize empty messages and context variables
    let messages = Vec::new();
    let context_variables = HashMap::new();
    let max_turns = 10;
    // Run the swarm with the agent
    let response = swarm
        .run(
            agent,        // Now safe to use agent here
            messages,
            context_variables,
            Some(model),  // Model override
            false,        // Do not stream
            false,        // Debug mode off
            max_turns,   // Max turns
        )
        .await?;

    // Print the response messages
    for message in response.messages {
        println!("{} {}: {}", message.name.unwrap_or_default(), message.role, message.content.unwrap_or_default());
        println!("--------------------------------\n");
    }

    Ok(())
}
