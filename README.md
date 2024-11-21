# rswarm: A Comprehensive Guide to AI Agent Interactions in Rust

Welcome, fellow Rustacean! If you’re aiming to integrate advanced AI agent interactions into your Rust applications, you’ve come to the right place. rswarm is a powerful and user-friendly library designed to simplify and enhance your AI development experience in Rust.

Embark on this journey with us as we explore how rswarm can empower your projects with intelligent agent capabilities.

## Introduction

rswarm is a Rust library crafted to streamline AI agent interactions, particularly when working with OpenAI’s API. It provides a robust framework for:

- **Managing AI agents with customizable behaviors**: Define agents with specific instructions and functions tailored to your application’s needs.
- **Executing conversations with advanced control**: Run complex dialogues with agents, controlling parameters like context, functions, and looping behaviors.
- **Integrating custom functions for extended capabilities**: Enhance agents with custom functions that can be called during conversations.
- **Handling streaming responses and error scenarios gracefully**: Receive real-time responses and implement robust error handling mechanisms.
- **Defining prompts and execution steps using XML**: Utilize XML to structure prompts, handoffs, function calls, and execution steps for more complex interactions.

Whether you’re building a chatbot, an AI assistant, or any application requiring intelligent dialogue, rswarm equips you with the tools to make it happen efficiently.

## Acknowledgments

This project, rswarm, is inspired by and extends the concepts introduced in the [Swarm](https://github.com/openai/swarm) framework developed by OpenAI. Swarm is an educational framework that explores ergonomic, lightweight multi-agent orchestration. It provides a foundation for agent coordination and execution through abstractions like Agents and handoffs, allowing for scalable and customizable solutions.

We would like to express our gratitude to the OpenAI team for their innovative work on Swarm, which has significantly influenced the development of rswarm. Special thanks to the core contributors of Swarm, including Ilan Bigio, James Hills, Shyamal Anadkat, Charu Jaiswal, Colin Jarvis, and Katia Gil Guzman, among others.

By building upon Swarm, rswarm aims to bring these powerful concepts into the Rust ecosystem, enhancing them to suit our specific needs and preferences. We hope to continue pushing the boundaries of what's possible with Rust and AI, inspired by the groundwork laid by OpenAI.

Feel free to explore the Swarm framework further, contribute to its development, or reach out with questions. Together, we can continue to innovate and expand the capabilities of AI agent interactions.

Happy coding!

## Installation

To get started with rswarm, you need to add it to your project’s dependencies. Ensure you have Rust and Cargo installed on your system.

### Adding rswarm to Your Project

In your `Cargo.toml` file, add:
```bash
cargo add rswarm
```
or  
```toml
[dependencies]
rswarm = { git = "https://github.com/socrates8300/rswarm.git" }
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

Let’s dive into a basic example to see rswarm in action.

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

First, define a function:

```rust
use rswarm::{AgentFunction, ContextVariables, ResultType};
use std::sync::Arc;

// Define the echo function
let echo_function = AgentFunction {
    name: "echo".to_string(),
    function: Arc::new(|args: ContextVariables| {
        let message = args.get("message").cloned().unwrap_or_default();
        Ok(ResultType::Value(message))
    }),
    accepts_context_variables: true,
};

// Add the function to the agent
let mut agent = Agent {
    name: "assistant".to_string(),
    model: "gpt-3.5-turbo".to_string(),
    instructions: Instructions::Text("You are a helpful assistant.".to_string()),
    functions: vec![echo_function],  // Add the function here
    function_call: Some("auto".to_string()),  // Allow the agent to call functions
    parallel_tool_calls: false,
};

// Now use the agent in conversation
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

In this example, we define an echo function that the agent can use to repeat messages. The agent will automatically decide when to use this function based on the conversation context.

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

### Defining Prompts and Execution Steps with XML

rswarm allows you to define complex interactions using XML, including prompts, handoffs, function calls, and execution steps. This feature enables you to structure conversations and control agent behavior in a more organized manner.

#### Using XML to Define Steps

You can embed XML within the agent’s instructions to define a sequence of steps for the agent to execute.

##### Example of XML Steps

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

#### How It Works

- **Step Number**: Indicates the order of execution.
- **Action**: Defines what the agent should do (run_once, loop).
- **Agent (Optional)**: Specifies which agent to use for the step.
- **Prompt**: The instruction or message for the agent.

#### Parsing XML Steps

The library provides functions to extract and parse these steps from the instructions.

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

// Extract XML steps
let (instructions_without_xml, xml_steps) = extract_xml_steps(instructions).unwrap();

// Parse the steps
let steps = if let Some(xml_content) = xml_steps {
    parse_steps_from_xml(&xml_content).unwrap()
} else {
    Steps { steps: Vec::new() }
};

// Now you can use `steps` in your conversation logic
```

#### Executing Steps

The Swarm’s `run()` method automatically handles the execution of steps defined in XML.

```rust
let response = swarm
    .run(
        agent.clone(),
        messages,
        context_variables,
        None,
        false,
        false,
        10
    )
    .await
    .expect("Failed to run the conversation with steps");
```

#### Step Actions

- **run_once**: Executes the step’s prompt once.
- **loop**: Repeats the step’s prompt until a termination condition is met.

#### Agent Handoffs

By specifying an agent in a step, you can switch agents during the conversation.

```xml
<step number="2" action="loop" agent="specialist_agent">
    <prompt>Provide detailed answers to the user's technical questions.</prompt>
</step>
```

In the above example, the conversation hands off to `specialist_agent` for step 2.

## Advanced Topics

Delve deeper into rswarm’s capabilities.

### Managing Multiple Agents

Handle complex applications with multiple agents.

#### Registering Agents

```rust
// Register the initial agent
swarm.agent_registry.insert(agent.name.clone(), agent.clone());

// Define another agent
let assistant_agent = Agent {
    name: "general_assistant".to_string(),
    model: "gpt-4".to_string(),
    instructions: Instructions::Text("You are a general-purpose assistant.".to_string()),
    functions: vec![],
    function_call: None,
    parallel_tool_calls: false,
};

// Register the new agent
swarm.agent_registry.insert(assistant_agent.name.clone(), assistant_agent.clone());
```

#### Switching Agents

```rust
let mut current_agent = swarm.get_agent_by_name("general_assistant")
    .expect("Agent not found");

if user_requests_specialized_info {
    current_agent = swarm.get_agent_by_name("specialist_agent")
        .expect("Agent not found");
}
```

### Custom Instruction Functions

Dynamic instructions adapt agent behavior in real-time.

#### Example

```rust
let custom_instructions = Instructions::Function(Arc::new(|context: ContextVariables| {
    let user_role = context.get("role").unwrap_or(&"user".to_string());
    format!("You are assisting a {}.", user_role)
}));

agent.instructions = custom_instructions;
```

### Loop Control and Execution Steps

Control complex conversation flows with loop control and execution steps.

#### Implementing Loop Control

When using the `loop` action in XML steps, rswarm handles loop execution and termination conditions.

- **Termination Condition**: The loop ends when the `context_variables` contain a key that matches a break condition (e.g., `"end_loop": "true"`).
- **Max Iterations**: Prevent infinite loops by setting `max_loop_iterations` in `SwarmConfig`.

```rust
let custom_config = SwarmConfig {
    max_loop_iterations: 5,
    ..Default::default()
};

let swarm = Swarm::builder()
    .with_config(custom_config)
    .build()
    .expect("Failed to create Swarm with custom configuration");
```

Within your function, you can set `context_variables` to signal loop termination.

```rust
let end_loop_function = AgentFunction {
    name: "end_loop".to_string(),
    function: Arc::new(|mut args: ContextVariables| {
        args.insert("end_loop".to_string(), "true".to_string());
        Ok(ResultType::ContextVariables(args))
    }),
    accepts_context_variables: true,
};

// Add the function to the agent
agent.functions.push(end_loop_function);
```

### Utilizing the Utility Functions

rswarm provides utility functions to assist with debugging and processing.

#### Debug Printing

```rust
use rswarm::debug_print;

debug_print(true, "This is a debug message.");
```

#### Merging Chunked Messages

When handling streaming responses, use `merge_chunk_message` to assemble messages.

```rust
use rswarm::{Message, merge_chunk_message};
use serde_json::json;

let mut message = Message {
    role: "assistant".to_string(),
    content: Some("Hello".to_string()),
    name: None,
    function_call: None,
};

let delta = json!({
    "content": " world!"
}).as_object().unwrap().clone();

merge_chunk_message(&mut message, &delta);
assert_eq!(message.content, Some("Hello world!".to_string()));
```

### Validation and Error Handling

Ensure your application handles errors gracefully by utilizing the validation functions provided by rswarm.

#### Validating API Requests

```rust
use rswarm::validation::validate_api_request;

validate_api_request(&agent, &messages, &None, 5)
    .expect("Validation failed");
```

#### Validating API URLs

```rust
use rswarm::validation::validate_api_url;

let api_url = "https://api.openai.com/v1/chat/completions";
validate_api_url(api_url, &swarm.config)
    .expect("Invalid API URL");
```

## Best Practices

- **Secure API Keys**: Use environment variables and avoid hardcoding sensitive information.
- **Handle Errors Gracefully**: Implement retry logic and provide user-friendly error messages.
- **Optimize Performance**: Adjust timeouts and retries based on application needs.
- **Keep Agents Modular**: Design agents with single responsibilities for easier maintenance.
- **Leverage Context**: Use context variables to enhance agent responses dynamically.
- **Use XML for Complex Flows**: Utilize XML definitions for structured and maintainable conversation flows.
- **Test Thoroughly**: Write tests to ensure your agents and functions work as expected.

## Conclusion

We’ve explored the landscape of rswarm, uncovering how it can elevate your Rust applications with intelligent AI interactions. From setting up a basic conversation to mastering advanced features like XML-defined execution steps, you’re now equipped to harness the full power of this library.

As you continue your development journey, remember that innovation thrives on experimentation. Don’t hesitate to explore new ideas, contribute to the rswarm community, and push the boundaries of what’s possible with Rust and AI.

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

- **Purpose**: Defines an AI assistant’s behavior.
- **Fields**:
  - `name`: Unique identifier.
  - `model`: AI model to use (e.g., "gpt-3.5-turbo").
  - `instructions`: Guides the agent’s responses.
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

- **Purpose**: Configures Swarm behavior.
- **Fields**:
  - `api_url`: The OpenAI API URL.
  - `request_timeout`: Max time for each API request.
  - `connect_timeout`: Max time to establish a connection.
  - `max_retries`: Max retry attempts for failed requests.
  - `max_loop_iterations`: Limits to prevent infinite loops.
  - `loop_control`: Settings for loop execution.
  - `api_settings`: Advanced API configurations.

### Instructions Enum

```rust
pub enum Instructions {
    Text(String),
    Function(Arc<dyn Fn(ContextVariables) -> String + Send + Sync>),
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
    Agent(Agent),
    ContextVariables(ContextVariables),
}
```

- **Purpose**: Represents the result of an agent function execution.

### Steps Struct

```rust
pub struct Steps {
    pub steps: Vec<Step>,
}
```

- **Purpose**: Represents a sequence of execution steps defined in XML.

### Step Struct

```rust
pub struct Step {
    pub number: usize,
    pub action: String,
    pub agent: Option<String>,
    pub prompt: String,
}
```

- **Purpose**: Defines a single execution step.
- **Fields**:
  - `number`: The sequence number of the step.
  - `action`: The action to perform (run_once, loop).
  - `agent`: Optional agent name for handoff.
  - `prompt`: The prompt to execute.

## License

This project is licensed under the MIT License.

## Acknowledgments

A heartfelt thank you to all contributors and the Rust community. Your support and collaboration make projects like rswarm possible.

Feel free to explore the library further, contribute to its development, or reach out with questions. Together, we can continue to push the boundaries of what’s possible with Rust and AI.

Happy coding!