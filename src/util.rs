use crate::types::{AgentFunction, FunctionCall, Message, Steps};
use crate::error::{SwarmError, SwarmResult};
use quick_xml::de::from_str as xml_from_str;
use regex::Regex;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn debug_print(debug: bool, message: &str) {
    if debug {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        println!("[{}] {}", timestamp, message);
    }
}

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

pub fn parse_steps_from_xml(xml_content: &str) -> SwarmResult<Steps> {
    xml_from_str(xml_content)
        .map_err(|e| SwarmError::XmlError(format!("Failed to parse XML steps: {}", e)))
}

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
