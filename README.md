# rswarm: A Comprehensive Guide to AI Agent Interactions in Rust

Welcome, fellow Rustacean! If you’re aiming to integrate advanced AI agent interactions into your Rust applications, you’ve come to the right place. rswarm is a powerful and user-friendly library designed to simplify and enhance your AI development experience in Rust.

Embark on this journey with us as we explore how rswarm can empower your projects with intelligent agent capabilities.

## Introduction

rswarm is a Rust library crafted to streamline AI agent interactions, particularly when working with OpenAI’s API. It provides a robust framework for:

- **Managing AI agents with customizable behaviors**: Define agents with specific instructions and functions tailored to your application’s needs.
- **Executing conversations with advanced control**: Run complex dialogues with agents, controlling parameters like context, functions, and looping behaviors.
- **Integrating custom functions for extended capabilities**: Enhance agents with custom functions that can be called during conversations.
- **Handling streaming responses and error scenarios gracefully**: Receive real-time, incremental responses using our streaming API and implement robust error handling.
- **Defining prompts and execution steps using XML**: Utilize XML to structure prompts, handoffs, function calls, and execution steps for more complex interactions.

Whether you’re building a chatbot, an AI assistant, or any application requiring intelligent dialogue, rswarm equips you with the tools to make it happen efficiently.

## Acknowledgments

This project, rswarm, is inspired by and extends the concepts introduced in the [Swarm](https://github.com/openai/swarm) framework developed by OpenAI. Swarm is an educational framework that explores ergonomic, lightweight multi-agent orchestration. It provides a foundation for agent coordination and execution through abstractions like Agents and handoffs, allowing for scalable and customizable solutions.

We would like to express our gratitude to the OpenAI team for their innovative work on Swarm, which has significantly influenced the development of rswarm. Special thanks to the core contributors of Swarm, including Ilan Bigio, James Hills, Shyamal Anadkat, Charu Jaiswal, Colin Jarvis, and Katia Gil Guzman, among others.

By building upon Swarm, rswarm aims to bring these powerful concepts into the Rust ecosystem, enhancing them to suit our specific needs and preferences. We hope to continue pushing the boundaries of what's possible with Rust and AI, inspired by the groundwork laid by OpenAI.

Feel free to explore the rswarm framework further, contribute to its development, or reach out with questions. Together, we can continue to innovate and expand the capabilities of AI agent interactions.

Happy coding!

## Installation

To get started with rswarm, you need to add it to your project’s dependencies. Ensure you have Rust and Cargo installed on your system.

### Adding rswarm to Your Project

In your `Cargo.toml` file, add:
```bash
cargo add rswarm
```

After updating `Cargo.toml`, fetch the dependencies by running:
```bash
cargo build
```

### Setting Up Environment Variables

rswarm relies on environment variables for configuration:

- **OPENAI_API_KEY (required)**: Your OpenAI API key.
- **OPENAI_API_URL (optional)**: Custom API URL if not using the default.

Set them in your shell or a `.env` file:
```bash
export OPENAI_API_KEY="your-api-key"
export OPENAI_API_URL="https://api.openai.com/v1/chat/completions"  # Optional
```

In your Rust application, load the `.env` file:
```rust
dotenv::dotenv().ok();
```

> **Note**: Keep your API key secure and avoid committing it to version control.

## Quick Start

Let’s dive into some examples to see rswarm in action.

### Creating a Swarm Instance

The `Swarm` struct is the heart of the library, managing API communication and agent interactions. You can create a Swarm instance using the builder pattern.

#### Using the Builder Pattern

```rust
use rswarm::Swarm;

let swarm = Swarm::builder()
    .build()
    .expect("Failed to create Swarm");
```

If you’ve set the `OPENAI_API_KEY` environment variable, you can omit the `.with_api_key()` method. If you prefer to pass the API key directly:

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

Instructions guide the agent’s behavior. They can be:

- **Static Text**: Fixed instructions provided as a `String`.
- **Dynamic Functions**: Generate instructions based on context using a closure.

Example of dynamic instructions:

```rust
use rswarm::{Instructions, ContextVariables};
use std::sync::Arc;

let dynamic_instructions = Instructions::Function(Arc::new(|context: ContextVariables| {
    format!(
        "You are a helpful assistant aware of the user's location: {}.",
        context.get("location").unwrap_or(&"unknown".to_string())
    )
}));
```

### Running Conversations

Let’s initiate a conversation with our agent.

#### Initiating a Chat (Batch Mode)

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

### Streaming Responses

For real-time applications, you can enable streaming to receive incremental responses.

#### Streaming Example

Instead of calling `run()`, create a `Streamer` from the Swarm’s client and API key:

```rust
use rswarm::stream::Streamer;
use futures_util::StreamExt;
use std::collections::HashMap;

let streamer = Streamer::new(swarm.client.clone(), swarm.api_key.clone());

let history = Vec::new();
let context_variables = HashMap::new();
let agent = agent.clone();  // The primary agent

println!("Starting streaming conversation output:");
let mut stream = streamer.stream_chat(&agent, &history, &context_variables, None, false);

// Process each streamed message as soon as it arrives.
while let Some(item) = stream.next().await {
    match item {
        Ok(message) => {
            println!(
                "{} {}: {}",
                message.name.as_deref().unwrap_or("Unknown"),
                message.role,
                message.content.as_deref().unwrap_or("")
            );
            println!("--------------------------------");
        }
        Err(e) => eprintln!("Stream error: {}", e),
    }
}
println!("Streaming conversation completed.");
```

In this example, the agent’s response is received incrementally via the Streamer.

## Deep Dive

Let’s explore rswarm in greater detail, uncovering its full potential.

### Swarm Configuration

Customize Swarm behavior using `SwarmConfig`.

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

Adjust parameters like timeouts and retries based on application needs.

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

let dynamic_instructions = Instructions::Function(Arc::new(|context: ContextVariables| {
    format!("You are a helpful assistant. The user's location is {}.", context.get("location").unwrap())
}));

agent.instructions = dynamic_instructions;
```

The agent tailors responses based on the context provided.

### Handling Function Calls

Agents can call functions during conversations to perform specific tasks.

#### Implementing Function Handling

Define a function, add it to the agent, and then proceed with a conversation:

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

agent.functions.push(echo_function);
agent.function_call = Some("auto".to_string());

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

### XML-Defined Prompts and Execution Steps

rswarm also allows for XML definitions to structure multi-step interactions.

#### Embedding XML Steps in Instructions

```xml
<steps>
  <step number="1" action="run_once">
    <prompt>Introduce yourself.</prompt>
  </step>
  <step number="2" action="loop" agent="assistant">
    <prompt>Answer the user's questions until they say 'goodbye'.</prompt>
  </step>
</steps>
```

#### Parsing and Executing XML Steps

```rust
use rswarm::{extract_xml_steps, parse_steps_from_xml, Steps};

let instructions = r#"
You are about to engage in a conversation.
<steps>
  <step number="1" action="run_once">
    <prompt>Introduce yourself.</prompt>
  </step>
  <step number="2" action="loop" agent="assistant">
    <prompt>Answer the user's questions until they say 'goodbye'.</prompt>
  </step>
</steps>
Proceed with the conversation.
"#;

let (instructions_without_xml, xml_steps) = extract_xml_steps(instructions).unwrap();

let steps = if let Some(xml_content) = xml_steps {
    parse_steps_from_xml(&xml_content).unwrap()
} else {
    Steps { steps: Vec::new() }
};
```

The Swarm’s `run()` method automatically handles the execution of steps defined in XML.

### Advanced Topics and Best Practices

- **Secure API Keys**: Use environment variables and avoid hardcoding sensitive information.
- **Handle Errors Gracefully**: Implement retry logic and provide user-friendly error messages.
- **Optimize Performance**: Adjust timeouts and retries based on application needs.
- **Keep Agents Modular**: Design agents with single responsibilities for easier maintenance.
- **Leverage Context**: Use context variables to enhance agent responses dynamically.
- **Use XML for Complex Flows**: Utilize XML definitions for structured and maintainable conversation flows.
- **Test Thoroughly**: Write tests (as provided in the examples) to ensure your agents and functions work as expected.

## Conclusion

We’ve explored the landscape of rswarm—how it simplifies AI agent interactions in Rust while providing advanced features like streaming responses, agent functions, and XML-based execution flows. Whether you’re just starting with AI in Rust or pushing the boundaries of complex interactions, rswarm provides a robust foundation to build upon.

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

**Purpose**: Manages API communication and agent interactions.
**Key Methods**:
- `run()`: Executes a conversation (batch mode or with XML-defined steps).
- `builder()`: Initializes a `SwarmBuilder`.
- `get_agent_by_name()`: Retrieves an agent from the registry.

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

**Purpose**: Defines an AI assistant’s behavior.
**Fields**:
- `name`: Unique identifier.
- `model`: AI model to be used (e.g., "gpt-3.5-turbo", "gpt-4").
- `instructions`: Guides the agent’s responses (static or dynamic).
- `functions`: Custom functions available to the agent.
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

**Purpose**: Enables agents to execute custom logic.
**Fields**:
- `name`: Identifier for the function.
- `function`: The executable function logic.
- `accepts_context_variables`: Indicates if context variables are used.

### SwarmConfig Struct

```rust
pub struct SwarmConfig {
    pub api_url: String,
    pub api_version: String,
    pub request_timeout: u64,
    pub connect_timeout: u64,
    pub max_retries: u32,
    pub max_loop_iterations: u32,
    pub valid_model_prefixes: Vec<String>,
    pub valid_api_url_prefixes: Vec<String>,
    pub loop_control: LoopControl,
    pub api_settings: ApiSettings,
}
```

**Purpose**: Configures Swarm behavior, including API endpoints, timeouts, and retry logic.

### Instructions Enum

```rust
pub enum Instructions {
    Text(String),
    Function(Arc<dyn Fn(ContextVariables) -> String + Send + Sync>),
}
```

**Purpose**: Provides static or dynamic instructions for agents.

### ContextVariables Type

```rust
pub type ContextVariables = HashMap<String, String>;
```

**Purpose**: Stores key-value pairs for dynamic context within conversations.

### ResultType Enum

```rust
pub enum ResultType {
    Value(String),
    Agent(Agent),
    ContextVariables(ContextVariables),
}
```

**Purpose**: Represents the result of an agent function execution.

### Streaming with Streamer

The new `Streamer` struct enables receiving real-time agent responses.

```rust
pub struct Streamer {
    client: Client,
    api_key: String,
}
```

**Key Method**:
- `stream_chat()`: Returns an asynchronous stream yielding incremental responses as `Message` items.

## License

This project is licensed under the MIT License.

## Acknowledgments

A heartfelt thank you to all contributors and the Rust community. Your support and collaboration make projects like rswarm possible.

Feel free to explore the library further, contribute to its development, or reach out with questions. Together, we can continue to push the boundaries of what’s possible with Rust and AI.

Happy coding!
