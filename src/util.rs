// ./src/util.rs
/// Utility functions for the Swarm library
///
/// This module provides various helper functions for debugging, message handling,
/// XML processing, and function conversion utilities.
use crate::types::{AgentFunction, FunctionCall, Message, Steps};
use crate::error::{SwarmError, SwarmResult};
use quick_xml::de::from_str as xml_from_str;
use regex::Regex;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

/// Prints debug messages with timestamps when debug mode is enabled
///
/// # Arguments
///
/// * `debug` - Boolean flag to enable/disable debug output
/// * `message` - The message to print
///
/// # Examples
///
/// ```rust
/// use rswarm::debug_print;
///
/// debug_print(true, "Processing request...");  // Prints: [1234567890] Processing request...
/// debug_print(false, "This won't print");     // Nothing printed
/// ```
pub fn debug_print(debug: bool, message: &str) {
    if debug {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| SwarmError::Other(format!("Failed to get system time: {}", e)))
            .unwrap_or_else(|_| std::time::Duration::from_secs(0))
            .as_secs();
        println!("[{}] {}", timestamp, message);
    }
}

/// Merges a delta message chunk into an existing message
///
/// Used for handling streaming responses where message content arrives in chunks.
/// Updates both content and function calls in the existing message.
///
/// # Arguments
///
/// * `message` - The existing message to update
/// * `delta` - The new chunk of message data to merge
///
/// # Examples
///
/// ```rust
/// use rswarm::{Message, merge_chunk_message};
/// use serde_json::json;
///
/// let mut message = Message {
///     role: "assistant".to_string(),
///     content: Some("Hello".to_string()),
///     name: None,
///     function_call: None,
/// };
///
/// let delta = json!({
///     "content": " world!"
/// }).as_object().unwrap().clone();
///
/// merge_chunk_message(&mut message, &delta);
/// assert_eq!(message.content, Some("Hello world!".to_string()));
/// ```
pub fn merge_chunk_message(message: &mut Message, delta: &serde_json::Map<String, Value>) {
    for (key, value) in delta {
        match key.as_str() {
            "content" => {
                if let Some(content) = value.as_str() {
                    if let Some(existing_content) = &mut message.content {
                        existing_content.push_str(content);
                    } else {
                        message.content = Some(content.to_string());
                    }
                }
            }
            "function_call" => {
                if let Ok(function_call) = serde_json::from_value::<FunctionCall>(value.clone()) {
                    message.function_call = Some(function_call);
                }
            }
            _ => {}
        }
    }
}

/// Converts an AgentFunction to a JSON representation
///
/// Transforms a function definition into the format expected by the OpenAI API.
///
/// # Arguments
///
/// * `func` - The AgentFunction to convert
///
/// # Returns
///
/// Returns a Result containing the JSON Value representation of the function
///
/// # Errors
///
/// Will return an error if JSON serialization fails
///
/// # Examples
///
/// ```rust
/// use rswarm::{AgentFunction, function_to_json};
/// use std::sync::Arc;
///
/// let func = AgentFunction {
///     name: "test_function".to_string(),
///     function: Arc::new(|_| Ok("result".into())),
///     accepts_context_variables: false,
/// };
///
/// let json = function_to_json(&func).unwrap();
/// assert_eq!(json["name"], "test_function");
/// ```
pub fn function_to_json(func: &AgentFunction) -> SwarmResult<Value> {
    let parameters = json!({
        "type": "object",
        "properties": {},
        "required": [],
    });

    Ok(json!({
        "name": func.name,
        "description": "",
        "parameters": parameters,
    }))
}

/// Parses XML content into a Steps structure
///
/// Converts XML-formatted step definitions into a structured Steps object
/// that can be executed by the agent.
///
/// # Arguments
///
/// * `xml_content` - The XML string containing step definitions
///
/// # Returns
///
/// Returns a Result containing the parsed Steps structure
///
/// # Errors
///
/// Will return an error if:
/// * XML parsing fails
/// * Required attributes are missing
/// * Step structure is invalid
///
/// # Examples
///
/// ```rust
/// use rswarm::parse_steps_from_xml;
///
/// let xml = r#"
///     <steps>
///         <step number="1" action="run_once">
///             <prompt>Hello, world!</prompt>
///         </step>
///     </steps>
/// "#;
///
/// let steps = parse_steps_from_xml(xml).unwrap();
/// assert_eq!(steps.steps.len(), 1);
/// ```
pub fn parse_steps_from_xml(xml_content: &str) -> SwarmResult<Steps> {
    xml_from_str(xml_content)
        .map_err(|e| SwarmError::XmlError(format!("Failed to parse XML steps: {}", e)))
}

/// Extracts XML step definitions from instructions text
///
/// Searches for and extracts XML step definitions from a larger text,
/// returning both the instructions without XML and the extracted XML content.
///
/// # Arguments
///
/// * `instructions` - The full instructions text containing potential XML steps
///
/// # Returns
///
/// Returns a tuple containing:
/// * The instructions text with XML removed
/// * Optional XML content if found
///
/// # Errors
///
/// Will return an error if:
/// * Regex pattern is invalid
/// * XML content is malformed
///
/// # Examples
///
/// ```rust
/// use rswarm::extract_xml_steps;
///
/// let instructions = r#"
/// Do the following:
/// <steps>
///     <step number="1" action="run_once">
///         <prompt>Hello</prompt>
///     </step>
/// </steps>
/// Then continue.
/// "#;
///
/// let (text, xml) = extract_xml_steps(instructions).unwrap();
/// assert!(text.contains("Do the following:"));
/// assert!(xml.is_some());
/// ```
pub fn extract_xml_steps(instructions: &str) -> SwarmResult<(String, Option<String>)> {
    let mut instructions_without_xml = instructions.to_string();
    let mut xml_steps = None;

    // Improved regex to be more robust
    let re = Regex::new(r"(?s)<steps\b[^>]*>.*?</steps>")
        .map_err(|e| SwarmError::Other(format!("Invalid regex pattern: {}", e)))?;

    if let Some(mat) = re.find(&instructions) {
        let xml_content = mat.as_str();
        instructions_without_xml.replace_range(mat.range(), "");
        xml_steps = Some(xml_content.to_string());
    }

    Ok((instructions_without_xml.trim().to_string(), xml_steps))
}
