#[cfg(test)]
mod tests {
    use crate::{Agent, Instructions, ToolCallExecution};
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn test_agent_serde_round_trip_for_text_agents() {
        let agent = Agent::new(
            "serde_agent",
            "gpt-4",
            Instructions::Text("Round-trip me".to_string()),
        )
        .expect("Failed to create agent")
        .with_tool_call_execution(ToolCallExecution::Parallel);

        let serialized = serde_json::to_value(&agent).expect("Agent should serialize");
        let deserialized: Agent =
            serde_json::from_value(serialized).expect("Agent should deserialize");

        assert_eq!(deserialized.name(), "serde_agent");
        assert_eq!(deserialized.model(), "gpt-4");
        assert_eq!(
            deserialized.tool_call_execution(),
            ToolCallExecution::Parallel
        );
        assert!(deserialized.functions().is_empty());
        match deserialized.instructions() {
            Instructions::Text(text) => assert_eq!(text, "Round-trip me"),
            _ => panic!("Expected text instructions after round-trip"),
        }
    }

    #[test]
    fn test_agent_deserialize_rejects_missing_instructions() {
        let error = serde_json::from_value::<Agent>(json!({
            "name": "serde_agent",
            "model": "gpt-4"
        }))
        .expect_err("Missing instructions should fail");

        assert!(error.to_string().contains("missing field"));
    }

    #[test]
    fn test_agent_deserialize_rejects_missing_name() {
        let error = serde_json::from_value::<Agent>(json!({
            "model": "gpt-4",
            "instructions": { "text": "Hello" },
            "functions": []
        }))
        .expect_err("Missing name should fail");

        assert!(error.to_string().contains("missing field"));
    }

    #[test]
    fn test_agent_deserialize_rejects_invalid_model() {
        let error = serde_json::from_value::<Agent>(json!({
            "name": "serde_agent",
            "model": "",
            "instructions": { "text": "Hello" },
            "functions": []
        }))
        .expect_err("Empty models should fail");

        assert!(error.to_string().contains("Agent model cannot be empty"));
    }

    #[test]
    fn test_agent_serialize_rejects_function_based_instructions() {
        let agent = Agent::new(
            "dynamic_agent",
            "gpt-4",
            Instructions::Function(Arc::new(|_| "dynamic".to_string())),
        )
        .expect("Failed to create agent");

        let error =
            serde_json::to_value(&agent).expect_err("Function instructions should not serialize");
        assert!(error.to_string().contains("function-based instructions"));
    }
}
