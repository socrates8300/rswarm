#[cfg(test)]
mod tests {
    use crate::types::{AgentFunction, ContextVariables, ResultType};
    use crate::{Agent, Instructions, Swarm, SwarmConfig, SwarmError};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;

    #[test]
    fn test_create_basic_agent() {
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("Basic test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        assert_eq!(agent.name, "test_agent");
        assert_eq!(agent.model, "gpt-4");
        match agent.instructions {
            Instructions::Text(text) => assert_eq!(text, "Basic test instructions"),
            _ => panic!("Expected Text instructions"),
        }
        assert!(agent.functions.is_empty());
        assert!(agent.function_call.is_none());
        assert!(!agent.parallel_tool_calls);
    }

    #[test]
    fn test_agent_with_function_instructions() {
        let instruction_fn =
            Arc::new(|_vars: ContextVariables| -> String { "Dynamic instructions".to_string() });

        let agent = Agent {
            name: "function_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Function(instruction_fn),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        // Test the function instructions
        let context = ContextVariables::new();
        match &agent.instructions {
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

        let agent = Agent {
            name: "function_enabled_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("Test with functions".to_string()),
            functions: vec![test_function],
            function_call: Some("auto".to_string()),
            parallel_tool_calls: true,
        };

        assert_eq!(agent.functions.len(), 1);
        assert_eq!(agent.functions[0].name, "test_function");
        assert_eq!(agent.functions[0].accepts_context_variables, false);
        assert_eq!(agent.function_call, Some("auto".to_string()));
        assert!(agent.parallel_tool_calls);
    }

    #[test]
    fn test_agent_in_swarm_registry() {
        let agent = Agent {
            name: "registry_test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let swarm = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent.clone())
            .build()
            .expect("Failed to build Swarm");

        assert!(swarm.agent_registry.contains_key(&agent.name));
        let registered_agent = swarm.agent_registry.get(&agent.name).unwrap();
        assert_eq!(registered_agent.name, "registry_test_agent");
        assert_eq!(registered_agent.model, "gpt-4");
    }

    #[test]
    fn test_agent_empty_name() {
        let agent = Agent {
            name: "".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        // Try to register the agent in a Swarm
        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

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
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "".to_string(),
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        // Try to register the agent in a Swarm
        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

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
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "invalid-model".to_string(), // Doesn't start with valid prefix
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

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
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("".to_string()), // Empty instructions
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        // Try to register the agent in a Swarm
        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

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
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "invalid-model".to_string(),
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

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
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "".to_string(),
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        assert!(matches!(result, Err(SwarmError::ValidationError(_))));
        if let Err(SwarmError::ValidationError(msg)) = result {
            assert!(msg.contains("Agent model cannot be empty"));
        }
    }

    #[test]
    fn test_agent_with_valid_model_prefix() {
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(), // Valid prefix
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        assert!(result.is_ok());
    }

    #[test]
    fn test_custom_model_prefix_validation() {
        // Create a custom config with specific model prefixes
        let config = SwarmConfig {
            valid_model_prefixes: vec!["custom-".to_string()],
            ..SwarmConfig::default()
        };

        let agent = Agent {
            name: "test_agent".to_string(),
            model: "custom-model".to_string(),
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_config(config)
            .with_agent(agent)
            .build();

        assert!(result.is_ok());
    }

    #[test]
    fn test_agent_with_valid_text_instructions() {
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("Valid test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        assert!(result.is_ok());
        if let Ok(swarm) = result {
            let stored_agent = swarm.agent_registry.get("test_agent").unwrap();
            match &stored_agent.instructions {
                Instructions::Text(text) => assert_eq!(text, "Valid test instructions"),
                _ => panic!("Expected Text instructions"),
            }
        }
    }

    #[test]
    fn test_agent_with_empty_text_instructions() {
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

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
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("    \n\t    ".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

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
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text(instructions.clone()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build();

        assert!(result.is_ok());
        if let Ok(swarm) = result {
            let stored_agent = swarm.agent_registry.get("test_agent").unwrap();
            match &stored_agent.instructions {
                Instructions::Text(text) => assert_eq!(text.as_str(), instructions.as_str()),
                _ => panic!("Expected Text instructions"),
            }
        }
    }

    #[test]
    fn test_basic_function_instructions() {
        let instruction_fn =
            Arc::new(|_: ContextVariables| -> String { "Basic function instructions".to_string() });

        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Function(instruction_fn),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let context = ContextVariables::new();
        match &agent.instructions {
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

        let agent = Agent {
            name: "context_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Function(instruction_fn),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let mut context = ContextVariables::new();
        context.insert("test_key".to_string(), "test_value".to_string());

        match &agent.instructions {
            Instructions::Function(f) => assert_eq!(f(context), "Context value: test_value"),
            _ => panic!("Expected Function instructions"),
        }
    }

    #[test]
    fn test_function_instructions_in_swarm() {
        let instruction_fn =
            Arc::new(|_: ContextVariables| -> String { "Swarm function instructions".to_string() });

        let agent = Agent {
            name: "swarm_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Function(instruction_fn),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let swarm = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_agent(agent)
            .build()
            .expect("Failed to build Swarm");

        let context = ContextVariables::new();
        let stored_agent = swarm.agent_registry.get("swarm_agent").unwrap();

        match &stored_agent.instructions {
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

        let agent = Agent {
            name: "complex_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Function(instruction_fn),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        // Test with empty context
        let empty_context = ContextVariables::new();
        match &agent.instructions {
            Instructions::Function(f) => assert_eq!(f(empty_context), "Default instructions"),
            _ => panic!("Expected Function instructions"),
        }

        // Test with partial context
        let mut partial_context = ContextVariables::new();
        partial_context.insert("name".to_string(), "Test Name".to_string());
        match &agent.instructions {
            Instructions::Function(f) => assert_eq!(f(partial_context), "Name: Test Name"),
            _ => panic!("Expected Function instructions"),
        }

        // Test with full context
        let mut full_context = ContextVariables::new();
        full_context.insert("name".to_string(), "Test Name".to_string());
        full_context.insert("role".to_string(), "Test Role".to_string());
        match &agent.instructions {
            Instructions::Function(f) => {
                assert_eq!(f(full_context), "Name: Test Name\nRole: Test Role")
            }
            _ => panic!("Expected Function instructions"),
        }
    }
}
