#[cfg(test)]
mod tests {
    use crate::types::{
        AgentFunction, ContextVariables, FunctionCallPolicy, ResultType, ToolCallExecution,
    };
    use crate::{Agent, Instructions, Swarm, SwarmConfig, SwarmError};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;

    fn text_agent(name: &str, model: &str, instructions: &str) -> Agent {
        Agent::new(name, model, Instructions::Text(instructions.to_string()))
            .expect("Failed to create test agent")
    }

    fn function_agent(
        name: &str,
        model: &str,
        instruction_fn: Arc<dyn Fn(ContextVariables) -> String + Send + Sync>,
    ) -> Agent {
        Agent::new(name, model, Instructions::Function(instruction_fn))
            .expect("Failed to create test agent")
    }

    #[test]
    fn test_create_basic_agent() {
        let agent = text_agent("test_agent", "gpt-4", "Basic test instructions");

        assert_eq!(agent.name(), "test_agent");
        assert_eq!(agent.model(), "gpt-4");
        match agent.instructions() {
            Instructions::Text(text) => assert_eq!(text, "Basic test instructions"),
            _ => panic!("Expected Text instructions"),
        }
        assert!(agent.functions().is_empty());
        assert_eq!(agent.function_call(), &FunctionCallPolicy::Disabled);
        assert_eq!(agent.tool_call_execution(), ToolCallExecution::Serial);
    }

    #[test]
    fn test_agent_with_function_instructions() {
        let instruction_fn =
            Arc::new(|_vars: ContextVariables| -> String { "Dynamic instructions".to_string() });

        let agent = function_agent("function_agent", "gpt-4", instruction_fn);

        // Test the function instructions
        let context = ContextVariables::new();
        match agent.instructions() {
            Instructions::Function(f) => assert_eq!(f(context), "Dynamic instructions"),
            _ => panic!("Expected Function instructions"),
        }
    }

    #[test]
    fn test_agent_with_functions() {
        let test_function =
            AgentFunction {
                name: "test_function".to_string(),
                function:
                    Arc::new(
                        |_: ContextVariables| -> Pin<
                            Box<dyn Future<Output = Result<ResultType, anyhow::Error>> + Send>,
                        > {
                            Box::pin(
                                async move { Ok(ResultType::Value("test result".to_string())) },
                            )
                        },
                    ),
                accepts_context_variables: false,
            };

        let agent = text_agent("function_enabled_agent", "gpt-4", "Test with functions")
            .with_functions(vec![test_function])
            .with_function_call_policy(FunctionCallPolicy::Auto)
            .with_tool_call_execution(ToolCallExecution::Parallel);

        assert_eq!(agent.functions().len(), 1);
        assert_eq!(agent.functions()[0].name, "test_function");
        assert!(!agent.functions()[0].accepts_context_variables);
        assert_eq!(agent.function_call(), &FunctionCallPolicy::Auto);
        assert_eq!(agent.tool_call_execution(), ToolCallExecution::Parallel);
    }

    #[test]
    fn test_agent_in_swarm_registry() {
        let agent = text_agent("registry_test_agent", "gpt-4", "Test instructions");

        let swarm = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent.clone())
            .build()
            .expect("Failed to build Swarm");

        assert!(swarm.agents().contains_key(agent.name()));
        let registered_agent = swarm.agents().get(agent.name()).unwrap();
        assert_eq!(registered_agent.name(), "registry_test_agent");
        assert_eq!(registered_agent.model(), "gpt-4");
    }

    #[test]
    fn test_agent_empty_name() {
        let result = Agent::new("", "gpt-4", Instructions::Text("Test instructions".to_string()));

        // Verify error
        assert!(result.is_err());
        match result {
            Err(SwarmError::ValidationError(msg)) => {
                assert!(msg.contains("Agent name cannot be empty"));
            }
            _ => panic!("Expected ValidationError for empty agent name"),
        }
    }

    #[test]
    fn test_agent_empty_model() {
        let result = Agent::new("test_agent", "", Instructions::Text("Test instructions".to_string()));

        // Verify error
        assert!(result.is_err());
        match result {
            Err(SwarmError::ValidationError(msg)) => {
                assert!(msg.contains("Agent model cannot be empty"));
            }
            _ => panic!("Expected ValidationError for empty model"),
        }
    }

    #[test]
    fn test_agent_invalid_model_prefix() {
        let agent = text_agent("test_agent", "invalid-model", "Test instructions");

        // Try to register the agent in a Swarm
        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        // Verify error
        assert!(result.is_err());
        match result {
            Err(SwarmError::ValidationError(msg)) => {
                assert!(msg.contains("Invalid model prefix"));
            }
            _ => panic!("Expected ValidationError for invalid model prefix"),
        }
    }

    #[test]
    fn test_agent_missing_instructions() {
        let result = Agent::new("test_agent", "gpt-4", Instructions::Text("".to_string()));

        // Verify error
        assert!(result.is_err());
        match result {
            Err(SwarmError::ValidationError(msg)) => {
                assert!(msg.contains("Agent instructions cannot be empty"));
            }
            _ => panic!("Expected ValidationError for empty instructions"),
        }
    }

    #[test]
    fn test_agent_with_invalid_model_prefix() {
        let agent = text_agent("test_agent", "invalid-model", "Test instructions");

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        assert!(matches!(result, Err(SwarmError::ValidationError(_))));
        if let Err(SwarmError::ValidationError(msg)) = result {
            assert!(msg.contains("Invalid model prefix"));
        }
    }

    #[test]
    fn test_agent_with_empty_model() {
        let result = Agent::new("test_agent", "", Instructions::Text("Test instructions".to_string()));

        assert!(matches!(result, Err(SwarmError::ValidationError(_))));
        if let Err(SwarmError::ValidationError(msg)) = result {
            assert!(msg.contains("Agent model cannot be empty"));
        }
    }

    #[test]
    fn test_agent_with_valid_model_prefix() {
        let agent = text_agent("test_agent", "gpt-4", "Test instructions");

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        assert!(result.is_ok());
    }

    #[test]
    fn test_custom_model_prefix_validation() {
        let mut config = SwarmConfig::default();
        config
            .set_valid_model_prefixes(vec!["custom-".to_string()])
            .unwrap();

        let agent = text_agent("test_agent", "custom-model", "Test instructions");

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_config(config)
            .with_agent(agent)
            .build();

        assert!(result.is_ok());
    }

    #[test]
    fn test_agent_with_valid_text_instructions() {
        let agent = text_agent("test_agent", "gpt-4", "Valid test instructions");

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        assert!(result.is_ok());
        if let Ok(swarm) = result {
            let stored_agent = swarm.agents().get("test_agent").unwrap();
            match stored_agent.instructions() {
                Instructions::Text(text) => assert_eq!(text, "Valid test instructions"),
                _ => panic!("Expected Text instructions"),
            }
        }
    }

    #[test]
    fn test_agent_with_empty_text_instructions() {
        let result = Agent::new("test_agent", "gpt-4", Instructions::Text("".to_string()));

        assert!(result.is_err());
        match result {
            Err(SwarmError::ValidationError(msg)) => {
                assert!(msg.contains("Agent instructions cannot be empty"));
            }
            _ => panic!("Expected ValidationError for empty instructions"),
        }
    }

    #[test]
    fn test_agent_with_whitespace_only_text_instructions() {
        let result = Agent::new(
            "test_agent",
            "gpt-4",
            Instructions::Text("    \n\t    ".to_string()),
        );

        assert!(result.is_err());
        match result {
            Err(SwarmError::ValidationError(msg)) => {
                assert!(msg.contains("Agent instructions cannot be empty"));
            }
            _ => panic!("Expected ValidationError for whitespace-only instructions"),
        }
    }

    #[test]
    fn test_agent_with_multiline_text_instructions() {
        let instructions = "Line 1\nLine 2\nLine 3".to_string();
        let agent = text_agent("test_agent", "gpt-4", &instructions);

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        assert!(result.is_ok());
        if let Ok(swarm) = result {
            let stored_agent = swarm.agents().get("test_agent").unwrap();
            match stored_agent.instructions() {
                Instructions::Text(text) => assert_eq!(text.as_str(), instructions.as_str()),
                _ => panic!("Expected Text instructions"),
            }
        }
    }

    #[test]
    fn test_basic_function_instructions() {
        let instruction_fn =
            Arc::new(|_: ContextVariables| -> String { "Basic function instructions".to_string() });

        let agent = function_agent("test_agent", "gpt-4", instruction_fn);

        let context = ContextVariables::new();
        match agent.instructions() {
            Instructions::Function(f) => assert_eq!(f(context), "Basic function instructions"),
            _ => panic!("Expected Function instructions"),
        }
    }

    #[test]
    fn test_function_instructions_with_context() {
        let instruction_fn = Arc::new(|vars: ContextVariables| -> String {
            match vars.get("test_key") {
                Some(value) => format!("Context value: {}", value),
                None => "No context value found".to_string(),
            }
        });

        let agent = function_agent("context_agent", "gpt-4", instruction_fn);

        let mut context = ContextVariables::new();
        context.insert("test_key".to_string(), "test_value".to_string());

        match agent.instructions() {
            Instructions::Function(f) => assert_eq!(f(context), "Context value: test_value"),
            _ => panic!("Expected Function instructions"),
        }
    }

    #[test]
    fn test_function_instructions_in_swarm() {
        let instruction_fn =
            Arc::new(|_: ContextVariables| -> String { "Swarm function instructions".to_string() });

        let agent = function_agent("swarm_agent", "gpt-4", instruction_fn);

        let swarm = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build()
            .expect("Failed to build Swarm");

        let context = ContextVariables::new();
        let stored_agent = swarm.agents().get("swarm_agent").unwrap();

        match stored_agent.instructions() {
            Instructions::Function(f) => assert_eq!(f(context), "Swarm function instructions"),
            _ => panic!("Expected Function instructions"),
        }
    }

    #[test]
    fn test_complex_function_instructions() {
        let instruction_fn = Arc::new(|vars: ContextVariables| -> String {
            let mut parts = Vec::new();

            if let Some(name) = vars.get("name") {
                parts.push(format!("Name: {}", name));
            }

            if let Some(role) = vars.get("role") {
                parts.push(format!("Role: {}", role));
            }

            if parts.is_empty() {
                "Default instructions".to_string()
            } else {
                parts.join("\n")
            }
        });

        let agent = function_agent("complex_agent", "gpt-4", instruction_fn);

        // Test with empty context
        let empty_context = ContextVariables::new();
        match agent.instructions() {
            Instructions::Function(f) => assert_eq!(f(empty_context), "Default instructions"),
            _ => panic!("Expected Function instructions"),
        }

        // Test with partial context
        let mut partial_context = ContextVariables::new();
        partial_context.insert("name".to_string(), "Test Name".to_string());
        match agent.instructions() {
            Instructions::Function(f) => assert_eq!(f(partial_context), "Name: Test Name"),
            _ => panic!("Expected Function instructions"),
        }

        // Test with full context
        let mut full_context = ContextVariables::new();
        full_context.insert("name".to_string(), "Test Name".to_string());
        full_context.insert("role".to_string(), "Test Role".to_string());
        match agent.instructions() {
            Instructions::Function(f) => {
                assert_eq!(f(full_context), "Name: Test Name\nRole: Test Role")
            }
            _ => panic!("Expected Function instructions"),
        }
    }
}
