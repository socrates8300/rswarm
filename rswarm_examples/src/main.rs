// src/main.rs

mod browse_docs;

use anyhow::{Context, Result};
use dotenv::dotenv;
use rswarm::{Agent, Instructions, Swarm};
use rswarm::types::AgentFunction;
use std::{
    collections::HashMap,
    env,
    fs,
    sync::Arc,
};
// use tokio::signal;
use crate::browse_docs::browse_rust_docs;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize environment variables from .env file
    dotenv().ok();

    // Retrieve configuration from environment variables
    let api_key = get_env_var("OPENAI_API_KEY")?;
    let model = env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());
    let prompt = read_prompt_file("prompt.txt")?;

    // Define the browse_docs function for agents
    let browse_docs_function = AgentFunction {
        name: "browse_docs".to_string(),
        function: Arc::new(browse_rust_docs),
        accepts_context_variables: false,
    };

    // Initialize agents
    let agents = initialize_agents(&model, &prompt, browse_docs_function)?;

    // Build the Swarm with the configured agents
    let swarm = Swarm::builder()
        .with_api_key(api_key)
        .with_agents(&agents)
        .build()
        .context("Failed to build Swarm")?;

    // Set up runtime parameters
    let messages = Vec::new();
    let context_variables = HashMap::new();
    let max_turns = 10;

    // Run the swarm with the primary agent
    let response = swarm
        .run(
            agents.get("Agent").expect("Agent not found").clone(),
            messages,
            context_variables,
            Some(model),
            false, // Do not stream
            false, // Debug mode off
            max_turns,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Swarm run failed: {}", e))?;

    // Display the response messages
    display_response(&response);

    // Gracefully handle shutdown signals
    wait_for_shutdown().await;

    Ok(())
}

/// Retrieves the value of the specified environment variable.
fn get_env_var(key: &str) -> Result<String> {
    env::var(key)
        .with_context(|| format!("{} environment variable not set", key))
}

/// Reads the prompt from the given file path.
fn read_prompt_file(path: &str) -> Result<String> {
    fs::read_to_string(path)
        .with_context(|| format!("Failed to read prompt file '{}'", path))
}

/// Initializes all agents required for the swarm.
fn initialize_agents(
    model: &str,
    prompt: &str,
    browse_docs_function: AgentFunction,
) -> Result<HashMap<String, Agent>> {
    let mut agents = HashMap::new();

    // Primary Agent
    agents.insert(
        "Agent".to_string(),
        Agent {
            name: "Agent".to_string(),
            model: model.to_string(),
            instructions: Instructions::Text(prompt.to_string()),
            functions: Vec::new(),
            function_call: None,
            parallel_tool_calls: true,
        },
    );

    // Agent Manager
    agents.insert(
        "AgentManager".to_string(),
        Agent {
            name: "AgentManager".to_string(),
            model: model.to_string(),
            instructions: Instructions::Text(
                "You are a CTO with 20 years of experience. Oversee the execution of tasks by other agents, conduct reviews, and provide feedback.".to_string(),
            ),
            functions: Vec::new(),
            function_call: None,
            parallel_tool_calls: true,
        },
    );

    // Documentation Browser Agent
    agents.insert(
        "DocBrowserAgent".to_string(),
        Agent {
            name: "DocBrowserAgent".to_string(),
            model: model.to_string(),
            instructions: Instructions::Text(
                "You can browse Rust documentation using the 'browse_docs' function. The 'browse_docs' function takes a string as an argument (query) and returns a string.".to_string(),
            ),
            functions: vec![browse_docs_function],
            function_call: None,
            parallel_tool_calls: false,
        },
    );

    Ok(agents)
}

/// Extends the Swarm builder with multiple agents.
trait SwarmBuilderExt {
    fn with_agents(self, agents: &HashMap<String, Agent>) -> Self;
}

// Refactor this is now part of the SwarmBuilder trait
impl SwarmBuilderExt for rswarm::core::SwarmBuilder {
    fn with_agents(mut self, agents: &HashMap<String, Agent>) -> Self {
        for agent in agents.values() {
            self = self.with_agent(agent.clone());
        }
        self
    }
}

/// Displays the response messages in a readable format.
fn display_response(response: &rswarm::Response) {
    for message in &response.messages {
        println!(
            "{} {}: {}",
            message.name.as_deref().unwrap_or("Unknown"),
            message.role,
            message.content.as_deref().unwrap_or("")
        );
        println!("--------------------------------\n");
    }
}

/// Waits for a shutdown signal to gracefully terminate the application.
async fn wait_for_shutdown() {
    println!("Workflow completed. \nPress Enter to exit...");
    let mut input = String::new();
    let _ = std::io::stdin().read_line(&mut input);
    println!("Shutting down gracefully...");
}