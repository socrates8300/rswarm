# rswarm: A Comprehensive Guide to AI Agent Interactions in Rust

Welcome, fellow Rustacean! If you're aiming to integrate advanced AI agent interactions into your Rust applications, you've come to the right place. **rswarm** is a powerful and user-friendly library designed to simplify and enhance your AI development experience in Rust.

Embark on this journey with us as we explore how **rswarm** can empower your projects with intelligent agent capabilities.

## Introduction

**rswarm** is a Rust library crafted to streamline AI agent interactions, particularly when working with OpenAI's API. It provides a robust framework for:

- **Managing AI agents with customizable behaviors**: Define agents with specific instructions and functions tailored to your application's needs.
- **Executing conversations with advanced control**: Run complex dialogues with agents, controlling parameters like context, functions, and looping behaviors.
- **Integrating custom functions for extended capabilities**: Enhance agents with custom functions that can be called during conversations.
- **Handling streaming responses and error scenarios gracefully**: Receive real-time responses and implement robust error handling mechanisms.

Whether you're building a chatbot, an AI assistant, or any application requiring intelligent dialogue, **rswarm** equips you with the tools to make it happen efficiently.

## Installation

To get started with **rswarm**, you need to add it to your project's dependencies. Ensure you have Rust and Cargo installed on your system.

### Adding rswarm to Your Project

In your `Cargo.toml` file, add:

```toml
[dependencies]
rswarm ={ git = "https://github.com/socrates8300/rswarm.git" }
```

After updating `Cargo.toml`, fetch the dependencies by running:

```sh
cargo build
```

### Setting Up Environment Variables

**rswarm** relies on environment variables for configuration:

- `OPENAI_API_KEY` (required): Your OpenAI API key.
- `OPENAI_API_URL` (optional): Custom API URL if not using the default.

Set them in your shell or a `.env` file:

```sh
export OPENAI_API_KEY="your-api-key"
export OPENAI_API_URL="https://api.openai.com/v1/chat/completions"  # Optional
```

In your Rust application, load the `.env` file:

```rust
dotenv::dotenv().ok();
```

> **Note**: Keep your API key secure and avoid committing it to version control.

## Quick Start

Let's dive into a basic example to see **rswarm** in action.

### Creating a Swarm Instance

The `Swarm` struct is the heart of the library, managing API communication and agent interactions. You can create a `Swarm` instance using the builder pattern.

#### Using the Builder Pattern

```rust
use rswarm::Swarm;

let swarm = Swarm::builder()
    .build()
    .expect("Failed to create Swarm");
```

If you've set the `OPENAI_API_KEY` environment variable, you can omit the `.with_api_key()` method. If you prefer to pass the API key directly:

```rust
let swarm = Swarm::builder()
    .with_api_key("your-api-key".to_string())
    .build()
    .expect("Failed to create Swarm");
```

### Defining Agents

An `Agent` encapsulates the behavior and capabilities of an AI assistant.

#### Creating an Agent

```rust
use rswarm::{Agent, Instructions};

let agent = Agent {
    name: "assistant".to_string(),
    model: "gpt-3.5-turbo".to_string(),
    instructions: Instructions::Text("You are a helpful assistant.".to_string()),
    functions: vec![],
    function_call: None,
    parallel_tool_calls: false,
};
```

#### Understanding Instructions

Instructions guide the agent's behavior. They can be:

- **Static Text**: Fixed instructions provided as a `String`.
- **Dynamic Functions**: Generate instructions based on context using a closure.

Example of dynamic instructions:

```rust
use rswarm::{Instructions, ContextVariables};
use std::sync::Arc;

let dynamic_instructions = Instructions::Function(Arc::new(|context: &ContextVariables| {
    format!(
        "You are a helpful assistant aware of the user's location: {}.",
        context.get("location").unwrap_or(&"unknown".to_string())
    )
}));
```

### Running Conversations

Let's initiate a conversation with our agent.

#### Initiating a Chat

```rust
use rswarm::{Message, ContextVariables};
use std::collections::HashMap;

let messages = vec![Message {
    role: "user".to_string(),
    content: Some("Hello, assistant!".to_string()),
    name: None,
    function_call: None,
}];

let context_variables = ContextVariables::new();  // An empty context

let response = swarm
    .run(
        agent.clone(),
        messages,
        context_variables,
        None,    // No model override
        false,   // Streaming disabled
        false,   // Debug mode off
        5        // Max turns
    )
    .await
    .expect("Failed to run the conversation");

for msg in response.messages {
    println!("{}: {}", msg.role, msg.content.unwrap_or_default());
}
```

The agent responds according to the instructions provided.

## Deep Dive

Let's explore **rswarm** in greater detail, uncovering its full potential.

### Swarm Configuration

Customize `Swarm` behavior using `SwarmConfig`.

#### Custom Configuration

```rust
use rswarm::{Swarm, SwarmConfig};

let custom_config = SwarmConfig {
    request_timeout: 60,
    max_retries: 5,
    ..Default::default()
};

let swarm = Swarm::builder()
    .with_config(custom_config)
    .build()
    .expect("Failed to create Swarm with custom configuration");
```

Adjust parameters like timeouts and retry strategies to suit your application's needs.

### Agent Functions

Agents can execute custom functions, extending their capabilities.

#### Defining Agent Functions

```rust
use rswarm::{AgentFunction, ContextVariables, ResultType};
use std::sync::Arc;

let echo_function = AgentFunction {
    name: "echo".to_string(),
    function: Arc::new(|args: ContextVariables| {
        let message = args.get("message").cloned().unwrap_or_default();
        Ok(ResultType::Value(message))
    }),
    accepts_context_variables: true,
};
```

#### Adding Functions to an Agent

```rust
agent.functions.push(echo_function);
agent.function_call = Some("auto".to_string());
```

With `function_call` set to `"auto"`, the agent decides when to use the functions.

### Context Variables

Context variables provide dynamic data to agents.

#### Using Context Variables

```rust
let mut context_variables = ContextVariables::new();
context_variables.insert("location".to_string(), "Berlin".to_string());

let dynamic_instructions = Instructions::Function(Arc::new(|context: &ContextVariables| {
    format!(
        "You are a helpful assistant. The user's location is {}.",
        context.get("location").unwrap()
    )
}));

agent.instructions = dynamic_instructions;
```

The agent tailors responses based on the context provided.

### Handling Function Calls

Agents can call functions during conversations to perform specific tasks.

#### Implementing Function Handling

```rust
use rswarm::Message;

let messages = vec![Message {
    role: "user".to_string(),
    content: Some("Repeat after me: Hello World!".to_string()),
    name: None,
    function_call: None,
}];

let response = swarm
    .run(
        agent.clone(),
        messages,
        ContextVariables::new(),
        None,
        false,
        false,
        5
    )
    .await
    .expect("Failed to run the conversation");

for msg in response.messages {
    println!("{}: {}", msg.role, msg.content.unwrap_or_default());
}
```

In this example, the agent uses the `echo` function to respond.

### Streaming Responses

For real-time applications, enable streaming to receive incremental responses.

#### Enabling Streaming

```rust
let response = swarm
    .run(
        agent.clone(),
        messages.clone(),
        context_variables.clone(),
        None,
        true,   // Enable streaming
        false,  // Debug mode off
        5
    )
    .await
    .expect("Failed to run the conversation");
```

### Error Handling and Retries

Robust error handling ensures a smooth user experience.

#### Configuring Retries

```rust
let custom_config = SwarmConfig {
    max_retries: 5,
    ..Default::default()
};

let swarm = Swarm::builder()
    .with_config(custom_config)
    .build()
    .expect("Failed to create Swarm with custom configuration");
```

#### Implementing Retry Logic

```rust
use rswarm::SwarmError;

match swarm.run(/* parameters */).await {
    Ok(response) => {
        // Process the response
    }
    Err(e) => {
        if e.is_retriable() {
            // Implement retry logic
        } else {
            eprintln!("An error occurred: {}", e);
        }
    }
}
```

## Advanced Topics

Delve deeper into **rswarm**'s capabilities.

### Managing Multiple Agents

Handle complex applications with multiple agents.

#### Registering Agents

```rust
swarm.agent_registry.insert(agent.name.clone(), agent.clone());

let assistant_agent = Agent {
    name: "general_assistant".to_string(),
    model: "gpt-4".to_string(),
    instructions: Instructions::Text("You are a general-purpose assistant.".to_string()),
    functions: vec![],
    function_call: None,
    parallel_tool_calls: false,
};

swarm.agent_registry.insert(assistant_agent.name.clone(), assistant_agent.clone());
```

#### Switching Agents

```rust
let mut current_agent = swarm.get_agent_by_name("general_assistant")
    .expect("Agent not found");

if user_requests_specialized_info {
    current_agent = swarm.get_agent_by_name("specialized_agent")
        .expect("Agent not found");
}
```

### Custom Instruction Functions

Dynamic instructions adapt agent behavior in real-time.

#### Example

```rust
let custom_instructions = Instructions::Function(Arc::new(|context: &ContextVariables| {
    let user_role = context.get("role").unwrap_or(&"user".to_string());
    format!("You are assisting a {}.", user_role)
}));

agent.instructions = custom_instructions;
```

### Loop Control and Execution Steps

Control complex conversation flows with loop control and execution steps.

#### Implementing Loop Control

While **rswarm** doesn't natively support defining execution steps with loop controls, you can implement custom looping logic in your application.

Example:

```rust
let max_iterations = 5;
for _ in 0..max_iterations {
    let response = swarm
        .run(
            agent.clone(),
            messages.clone(),
            context_variables.clone(),
            None,
            false,
            false,
            1
        )
        .await
        .expect("Failed to run the conversation");

    // Process the response
    // ...

    if some_termination_condition {
        break;
    }
}
```

## Best Practices

- **Secure API Keys**: Use environment variables and avoid hardcoding sensitive information.
- **Handle Errors Gracefully**: Implement retry logic and provide user-friendly error messages.
- **Optimize Performance**: Adjust timeouts and retries based on application needs.
- **Keep Agents Modular**: Design agents with single responsibilities for easier maintenance.
- **Leverage Context**: Use context variables to enhance agent responses dynamically.

## Conclusion

We've explored the landscape of **rswarm**, uncovering how it can elevate your Rust applications with intelligent AI interactions. From setting up a basic conversation to mastering advanced features, you're now equipped to harness the full power of this library.

As you continue your development journey, remember that innovation thrives on experimentation. Don't hesitate to explore new ideas, contribute to the **rswarm** community, and push the boundaries of what's possible with Rust and AI.

Happy coding!

## Appendix: API Reference

### Swarm Struct

```rust
pub struct Swarm {
    pub client: Client,
    pub api_key: String,
    pub agent_registry: HashMap<String, Agent>,
    pub config: SwarmConfig,
}
```

- **Purpose**: Manages API communication and agent interactions.
- **Key Methods**:
  - `run()`: Executes a conversation.
  - `builder()`: Initializes a `SwarmBuilder`.

### Agent Struct

```rust
pub struct Agent {
    pub name: String,
    pub model: String,
    pub instructions: Instructions,
    pub functions: Vec<AgentFunction>,
    pub function_call: Option<String>,
    pub parallel_tool_calls: bool,
}
```

- **Purpose**: Defines an AI assistant's behavior.
- **Fields**:
  - `name`: Unique identifier.
  - `model`: AI model to use (e.g., `"gpt-3.5-turbo"`).
  - `instructions`: Guides the agent's responses.
  - `functions`: Custom functions the agent can call.
  - `function_call`: Determines when functions are called.
  - `parallel_tool_calls`: Enables parallel execution of functions.

### AgentFunction Struct

```rust
pub struct AgentFunction {
    pub name: String,
    pub function: Arc<dyn Fn(ContextVariables) -> Result<ResultType> + Send + Sync>,
    pub accepts_context_variables: bool,
}
```

- **Purpose**: Enables agents to execute custom logic.
- **Fields**:
  - `name`: Function identifier.
  - `function`: The executable function.
  - `accepts_context_variables`: Indicates if it uses context variables.

### SwarmConfig Struct

```rust
pub struct SwarmConfig {
    pub api_url: String,
    pub request_timeout: u64,
    pub connect_timeout: u64,
    pub max_retries: u32,
    pub max_loop_iterations: u32,
    pub valid_model_prefixes: Vec<String>,
    pub valid_api_url_prefixes: Vec<String>,
    // Other fields...
}
```

- **Purpose**: Configures `Swarm` behavior.
- **Fields**:
  - `api_url`: The OpenAI API URL.
  - `request_timeout`: Max time for each API request.
  - `connect_timeout`: Max time to establish a connection.
  - `max_retries`: Max retry attempts for failed requests.
  - `max_loop_iterations`: Limits to prevent infinite loops.

### Instructions Enum

```rust
pub enum Instructions {
    Text(String),
    Function(Arc<dyn Fn(&ContextVariables) -> String + Send + Sync>),
}
```

- **Purpose**: Defines agent instructions.
- **Variants**:
  - `Text`: Static instructions.
  - `Function`: Dynamic instructions based on context.

### ContextVariables Type

```rust
pub type ContextVariables = HashMap<String, String>;
```

- **Purpose**: Stores key-value pairs for context within conversations.

### ResultType Enum

```rust
pub enum ResultType {
    Value(String),
    // Other variants...
}
```

- **Purpose**: Represents the result of an agent function execution.

## License

This project is licensed under the [MIT License](LICENSE).

## Acknowledgments

A heartfelt thank you to all contributors and the Rust community. Your support and collaboration make projects like **rswarm** possible.

Feel free to explore the library further, contribute to its development, or reach out with questions. Together, we can continue to push the boundaries of what's possible with Rust and AI.

---

Happy coding!