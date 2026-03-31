// src/main.rs

mod browse_docs;

use anyhow::{Context, Result};
use dotenvy::dotenv;
use rswarm::types::AgentFunction;
use rswarm::{Agent, Instructions, Message, Swarm, ToolCallExecution};
use std::future::Future;
use std::pin::Pin;
use std::{collections::HashMap, env, fs, sync::Arc};
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
    let browse_docs_function = AgentFunction::new(
        "browse_docs",
        Arc::new(
            |args| -> Pin<
                Box<
                    dyn Future<
                            Output = std::result::Result<
                                rswarm::types::ResultType,
                                rswarm::SwarmError,
                            >,
                        > + Send,
                >,
            > {
                Box::pin(async move {
                    browse_rust_docs(args).map_err(|e| rswarm::SwarmError::Other(e.to_string()))
                })
            },
        ),
        false,
    )
    .expect("browse_docs function name is valid");

    // Initialize agents
    let agents = initialize_agents(&model, &prompt, browse_docs_function)?;

    // Build the Swarm with the configured agents
    let swarm = Swarm::builder()
        .with_api_key(api_key)
        .with_agents(&agents)
        .build()
        .context("Failed to build Swarm")?;

    // Set up runtime parameters
    let messages = vec![
        Message::user("Hello! Please help me browse Rust documentation.")
            .expect("failed to create initial message"),
    ];
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
    env::var(key).with_context(|| format!("{} environment variable not set", key))
}

/// Reads the prompt from the given file path.
fn read_prompt_file(path: &str) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("Failed to read prompt file '{}'", path))
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
        Agent::new("Agent", model, Instructions::Text(prompt.to_string()))?
            .with_tool_call_execution(ToolCallExecution::Parallel),
    );

    // Agent Manager
    agents.insert(
        "AgentManager".to_string(),
        Agent::new(
            "AgentManager",
            model,
            Instructions::Text(
                "You are a CTO with 20 years of experience. Oversee the execution of tasks by other agents, conduct reviews, and provide feedback.".to_string(),
            ),
        )?
        .with_tool_call_execution(ToolCallExecution::Parallel),
    );

    // Documentation Browser Agent
    agents.insert(
        "DocBrowserAgent".to_string(),
        Agent::new(
            "DocBrowserAgent",
            model,
            Instructions::Text(
                "You can browse Rust documentation using the 'browse_docs' function. The 'browse_docs' function takes a string as an argument (query) and returns a string.".to_string(),
            ),
        )?
        .with_functions(vec![browse_docs_function]),
    );

    Ok(agents)
}

/// Extends the Swarm builder with multiple agents.
pub trait SwarmBuilderExt {
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
            message.name().unwrap_or("Unknown"),
            message.role(),
            message.content().unwrap_or("")
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
