#[cfg(test)]
mod tests {
    use crate::{Swarm, SwarmConfig, Agent, Instructions, SwarmError};
    use crate::constants::{OPENAI_DEFAULT_API_URL, MIN_REQUEST_TIMEOUT, MAX_REQUEST_TIMEOUT};
    use crate::types::ApiSettings;

#[test]
fn test_valid_swarm_initialization() {
    // Create custom config
    let config = SwarmConfig {
        api_url: "https://api.openai.com/v1".to_string(),
        api_version: "v1".to_string(),
        request_timeout: 30,
        connect_timeout: 10,
        api_settings: ApiSettings::default(),
        max_retries: 3,
        max_loop_iterations: 10,
        valid_model_prefixes: vec!["gpt-".to_string()],
        valid_api_url_prefixes: vec!["https://api.openai.com".to_string()],
        loop_control: Default::default(),
    };

    // Create test agent
    let agent = Agent {
        name: "test_agent".to_string(),
        model: "gpt-4".to_string(),
        instructions: Instructions::Text("Test instructions".to_string()),
        functions: vec![],
        function_call: None,
        parallel_tool_calls: false,
    };

    // Initialize Swarm using builder pattern
    let swarm = Swarm::builder()
        .with_api_key("sk-test123456789".to_string())
        .with_config(config.clone())
        .with_agent(agent.clone())
        .build()
        .expect("Failed to create Swarm");

    // Verify fields are correctly set
    assert_eq!(swarm.api_key, "sk-test123456789");
    assert_eq!(swarm.config.api_url, config.api_url);
    assert_eq!(swarm.config.request_timeout, config.request_timeout);
    assert_eq!(swarm.config.connect_timeout, config.connect_timeout);
    assert_eq!(swarm.config.max_retries, config.max_retries);
    assert!(swarm.agent_registry.contains_key("test_agent"));
    assert_eq!(swarm.agent_registry["test_agent"].name, agent.name);
    assert_eq!(swarm.agent_registry["test_agent"].model, agent.model);
}

#[test]
fn test_default_swarm_initialization() {
    // Test default initialization using environment variable
    std::env::set_var("OPENAI_API_KEY", "sk-test123456789");

    let swarm = Swarm::default();

    // Verify default values
    assert_eq!(swarm.api_key, "sk-test123456789");
    assert!(swarm.agent_registry.is_empty());
    assert_eq!(swarm.config.api_url, OPENAI_DEFAULT_API_URL);

    // Clean up
    std::env::remove_var("OPENAI_API_KEY");
}

#[test]
fn test_missing_api_key() {
    // Remove API key from environment if present
    std::env::remove_var("OPENAI_API_KEY");

    // Attempt to create Swarm without API key
    let result = Swarm::builder().build();

    // Verify error
    assert!(result.is_err());
    match result {
        Err(SwarmError::ValidationError(msg)) => {
            assert!(msg.contains("API key must be set"));
        }
        _ => panic!("Expected ValidationError for missing API key"),
    }
}

#[test]
fn test_invalid_configurations() {
    // Test cases with invalid configurations
    let test_cases = vec![
        (
            SwarmConfig {
                request_timeout: 0,
                ..SwarmConfig::default()
            },
            "request_timeout must be greater than 0"
        ),
        (
            SwarmConfig {
                connect_timeout: 0,
                ..SwarmConfig::default()
            },
            "connect_timeout must be greater than 0"
        ),
        (
            SwarmConfig {
                max_retries: 0,
                ..SwarmConfig::default()
            },
            "max_retries must be greater than 0"
        ),
        (
            SwarmConfig {
                valid_model_prefixes: vec![],
                ..SwarmConfig::default()
            },
            "valid_model_prefixes cannot be empty"
        ),
        (
            SwarmConfig {
                request_timeout: MIN_REQUEST_TIMEOUT - 1,
                ..SwarmConfig::default()
            },
            "request_timeout must be between"
        ),
        (
            SwarmConfig {
                request_timeout: MAX_REQUEST_TIMEOUT + 1,
                ..SwarmConfig::default()
            },
            "request_timeout must be between"
        ),
    ];

    for (config, expected_error) in test_cases {
        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_config(config)
            .build();

        assert!(result.is_err());
        match result {
            Err(SwarmError::ValidationError(msg)) => {
                assert!(
                    msg.contains(expected_error),
                    "Expected error message containing '{}', got '{}'",
                    expected_error,
                    msg
                );
            }
            _ => panic!("Expected ValidationError for invalid configuration"),
        }
    }
    }
}
