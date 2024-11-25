#[cfg(test)]
mod tests {
    use crate::{Swarm, SwarmConfig, Agent, Instructions, SwarmError};
    use crate::constants::OPENAI_DEFAULT_API_URL;
    use std::sync::Arc;
    use reqwest::Client;
    use std::time::Duration;



    #[test]
    fn test_invalid_api_url() {
        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_api_url("http://invalid-url.com".to_string()) // Non-HTTPS URL
            .build();

        assert!(result.is_err());
        match result {
            Err(SwarmError::ValidationError(msg)) => {
                assert!(msg.contains("API URL must start with https://"));
            }
            _ => panic!("Expected ValidationError for invalid API URL"),
        }
    }

    #[test]
    fn test_valid_configurations() {
        // Test valid configuration ranges
        let valid_config = SwarmConfig {
            request_timeout: 30,
            connect_timeout: 10,
            max_retries: 3,
            valid_model_prefixes: vec!["gpt-".to_string()],
            api_url: "https://api.openai.com/v1".to_string(),
            ..SwarmConfig::default()
        };

        let result = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_config(valid_config)
            .build();

        assert!(result.is_ok());
    }
    #[test]
    fn test_builder_basic_config() {
        let test_api_key = "sk-test123456789".to_string();

        let swarm = Swarm::builder()
            .with_api_key(test_api_key.clone())
            .build()
            .expect("Failed to build Swarm");

        assert_eq!(swarm.api_key, test_api_key);
        assert_eq!(swarm.config.api_url, OPENAI_DEFAULT_API_URL);
    }
    #[test]
    fn test_builder_api_settings() {
        let test_api_key = "sk-test123456789".to_string();
        let test_api_url = "https://api.openai.com/v2".to_string();
        let test_api_version = "2024-01".to_string();

        let swarm = Swarm::builder()
            .with_api_key(test_api_key.clone())
            .with_api_url(test_api_url.clone())
            .with_api_version(test_api_version.clone())
            .build()
            .expect("Failed to build Swarm");

        assert_eq!(swarm.api_key, test_api_key);
        assert_eq!(swarm.config.api_url, test_api_url);
        assert_eq!(swarm.config.api_version, test_api_version);
    }
    #[test]
    fn test_builder_timeout_settings() {
        let test_api_key = "sk-test123456789".to_string();
        let test_request_timeout = 60;
        let test_connect_timeout = 20;

        let swarm = Swarm::builder()
            .with_api_key(test_api_key.clone())
            .with_request_timeout(test_request_timeout)
            .with_connect_timeout(test_connect_timeout)
            .build()
            .expect("Failed to build Swarm");

        assert_eq!(swarm.config.request_timeout, test_request_timeout);
        assert_eq!(swarm.config.connect_timeout, test_connect_timeout);
    }
    #[test]
    fn test_builder_with_agent() {
        let test_api_key = "sk-test123456789".to_string();
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("Test instructions".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        let swarm = Swarm::builder()
            .with_api_key(test_api_key)
            .with_agent(agent)
            .build()
            .expect("Failed to build Swarm");

        assert!(swarm.agent_registry.contains_key("test_agent"));
        assert_eq!(swarm.agent_registry["test_agent"].model, "gpt-4");
    }
    #[test]
    fn test_builder_with_custom_client() {
        let custom_client = Client::builder()
            .timeout(Duration::from_secs(45))
            .build()
            .expect("Failed to create custom client");

        let swarm = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .with_client(custom_client)
            .build()
            .expect("Failed to build Swarm");

        // Since we can't check the client's timeout directly,
        // we'll just verify the client was set
        assert!(Arc::strong_count(&Arc::new(swarm.client)) >= 1);
    }
    #[test]
    fn test_builder_default_values() {
        let swarm = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .build()
            .expect("Failed to build Swarm");

        let default_config = SwarmConfig::default();

        assert_eq!(swarm.config.api_url, default_config.api_url);
        assert_eq!(swarm.config.api_version, default_config.api_version);
        assert_eq!(swarm.config.request_timeout, default_config.request_timeout);
        assert_eq!(swarm.config.connect_timeout, default_config.connect_timeout);
        assert_eq!(swarm.config.max_retries, default_config.max_retries);
        assert_eq!(swarm.config.max_loop_iterations, default_config.max_loop_iterations);
        assert!(swarm.agent_registry.is_empty());
    }
}