#[cfg(test)]
mod tests {
    use crate::constants::OPENAI_DEFAULT_API_URL;
    use crate::{Agent, Instructions, Swarm, SwarmConfig, SwarmError};
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
        let mut valid_config = SwarmConfig::default();
        valid_config.set_request_timeout(30).unwrap();
        valid_config.set_connect_timeout(10).unwrap();
        valid_config.set_max_retries(3).unwrap();
        valid_config
            .set_valid_model_prefixes(vec!["gpt-".to_string()])
            .unwrap();
        valid_config
            .set_api_url("https://api.openai.com/v1".to_string())
            .unwrap();

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

        assert_eq!(swarm.api_key().as_str(), test_api_key);
        assert_eq!(swarm.config().api_url(), OPENAI_DEFAULT_API_URL);
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

        assert_eq!(swarm.api_key().as_str(), test_api_key);
        assert_eq!(swarm.config().api_url(), test_api_url);
        assert_eq!(swarm.config().api_version(), test_api_version);
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

        assert_eq!(swarm.config().request_timeout(), test_request_timeout);
        assert_eq!(swarm.config().connect_timeout(), test_connect_timeout);
    }
    #[test]
    fn test_builder_with_agent() {
        let test_api_key = "sk-test123456789".to_string();
        let agent = Agent::new(
            "test_agent",
            "gpt-4",
            Instructions::Text("Test instructions".to_string()),
        )
        .expect("Failed to create Agent");

        let swarm = Swarm::builder()
            .with_api_key(test_api_key)
            .with_agent(agent)
            .build()
            .expect("Failed to build Swarm");

        assert!(swarm.agents().contains_key("test_agent"));
        assert_eq!(swarm.agents()["test_agent"].model(), "gpt-4");
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

        let _ = swarm.client();
    }
    #[test]
    fn test_builder_default_values() {
        let swarm = Swarm::builder()
            .with_api_key("sk-test123456789".to_string())
            .build()
            .expect("Failed to build Swarm");

        let default_config = SwarmConfig::default();

        assert_eq!(swarm.config().api_url(), default_config.api_url());
        assert_eq!(swarm.config().api_version(), default_config.api_version());
        assert_eq!(
            swarm.config().request_timeout(),
            default_config.request_timeout()
        );
        assert_eq!(
            swarm.config().connect_timeout(),
            default_config.connect_timeout()
        );
        assert_eq!(swarm.config().max_retries(), default_config.max_retries());
        assert_eq!(
            swarm.config().max_loop_iterations(),
            default_config.max_loop_iterations()
        );
        assert!(swarm.agents().is_empty());
    }
}
