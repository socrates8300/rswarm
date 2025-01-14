// ./src/util.rs
use crate::error::{SwarmError, SwarmResult};
/// Utility functions for the Swarm library
///
/// This module provides various helper functions for debugging, message handling,
/// XML processing, and function conversion utilities.
use crate::types::{AgentFunction, FunctionCall, Message, Steps};
use quick_xml::de::from_str as xml_from_str;
use regex::Regex;
use serde_json::{json, Value};

/// Prints debug messages when debug mode is enabled
///
/// Prefixes debug messages with "[DEBUG]" for easy identification in logs.
///
/// # Arguments
///
/// * `debug` - Boolean flag to enable/disable debug output
/// * `message` - The message to print
///
///
pub fn debug_print(debug: bool, message: &str) {
    if debug {
        println!("[DEBUG]: {}", message);
    }
}

/// Merges a delta message chunk into an existing message
///
/// Used for handling streaming responses where message content arrives in chunks.
/// Updates both content and function calls in the existing message. Particularly
/// useful when processing streaming responses from the OpenAI API.
///
/// # Arguments
///
/// * `message` - The existing message to update
/// * `delta` - The new chunk of message data to merge
///
///
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
///
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
///
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
