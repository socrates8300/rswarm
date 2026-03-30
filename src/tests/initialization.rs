#[cfg(test)]
mod tests {
    use crate::constants::{MAX_REQUEST_TIMEOUT, MIN_REQUEST_TIMEOUT, OPENAI_DEFAULT_API_URL};
    use crate::{Agent, Instructions, Swarm, SwarmConfig, SwarmError};
    use std::sync::Mutex;

    // Serializes tests that read/write OPENAI_API_KEY to prevent races.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Verifies that SwarmConfig::default() doesn't panic.
    /// If any constant in constants.rs is changed to an invalid value this test
    /// will catch it before the .expect() calls in Default::default() fire at runtime.
    #[test]
    fn test_swarm_config_default_is_valid() {
        let config = SwarmConfig::default();
        assert_eq!(config.api_url(), OPENAI_DEFAULT_API_URL);
        assert!(config.request_timeout() > 0);
        assert!(config.connect_timeout() > 0);
        assert!(config.max_retries() > 0);
    }

    #[test]
    fn test_valid_swarm_initialization() {
        let mut config = SwarmConfig::default();
        config
            .set_api_url("https://api.openai.com/v1".to_string())
            .unwrap();
        config.set_api_version("v1".to_string()).unwrap();
        config.set_request_timeout(30).unwrap();
        config.set_connect_timeout(10).unwrap();
        config.set_max_retries(3).unwrap();
        config.set_max_loop_iterations(10).unwrap();
        config
            .set_valid_model_prefixes(vec!["gpt-".to_string()])
            .unwrap();
        config
            .set_valid_api_url_prefixes(vec!["https://api.openai.com".to_string()])
            .unwrap();

        // Create test agent
        let agent = Agent::new(
            "test_agent",
            "gpt-4",
            Instructions::Text("Test instructions".to_string()),
        )
        .expect("Failed to create test agent");

        // Initialize Swarm using builder pattern
        let swarm = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_config(config.clone())
            .with_agent(agent.clone())
            .build()
            .expect("Failed to create Swarm");

        // Verify fields are correctly set
        assert_eq!(swarm.api_key().as_str(), "sk-test123456789");
        assert_eq!(swarm.config().api_url(), config.api_url());
        assert_eq!(swarm.config().request_timeout(), config.request_timeout());
        assert_eq!(swarm.config().connect_timeout(), config.connect_timeout());
        assert_eq!(swarm.config().max_retries(), config.max_retries());
        assert!(swarm.agents().contains_key("test_agent"));
        assert_eq!(swarm.agents()["test_agent"].name(), agent.name());
        assert_eq!(swarm.agents()["test_agent"].model(), agent.model());
    }

    #[test]
    fn test_default_swarm_initialization() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("OPENAI_API_KEY", "sk-test123456789");

        let swarm = Swarm::default();

        assert_eq!(swarm.api_key().as_str(), "sk-test123456789");
        assert!(swarm.agents().is_empty());
        assert_eq!(swarm.config().api_url(), OPENAI_DEFAULT_API_URL);

        std::env::remove_var("OPENAI_API_KEY");
    }

    #[test]
    fn test_missing_api_key() {
        let _guard = ENV_LOCK.lock().unwrap();
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
        let test_cases = vec![
            (
                Swarm::builder()
                    .with_api_key("sk-test123456789".to_string())
                    .with_request_timeout(0)
                    .build(),
                "request_timeout must be greater than 0",
            ),
            (
                Swarm::builder()
                    .with_api_key("sk-test123456789".to_string())
                    .with_connect_timeout(0)
                    .build(),
                "connect_timeout must be greater than 0",
            ),
            (
                Swarm::builder()
                    .with_api_key("sk-test123456789".to_string())
                    .with_max_retries(0)
                    .build(),
                "max_retries must be greater than 0",
            ),
            (
                Swarm::builder()
                    .with_api_key("sk-test123456789".to_string())
                    .with_valid_model_prefixes(vec![])
                    .build(),
                "valid_model_prefixes cannot be empty",
            ),
            (
                Swarm::builder()
                    .with_api_key("sk-test123456789".to_string())
                    .with_request_timeout(MIN_REQUEST_TIMEOUT - 1)
                    .build(),
                "request_timeout must be between",
            ),
            (
                Swarm::builder()
                    .with_api_key("sk-test123456789".to_string())
                    .with_request_timeout(MAX_REQUEST_TIMEOUT + 1)
                    .build(),
                "request_timeout must be between",
            ),
        ];

        for (result, expected_error) in test_cases {
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
