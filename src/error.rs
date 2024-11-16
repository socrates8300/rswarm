use thiserror::Error;

#[derive(Error, Debug)]
pub enum SwarmError {
    #[error("API error: {0}")]
    ApiError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Agent error: {0}")]
    AgentError(String),

    #[error("Invalid input: {0}")]
    ValidationError(String),

    #[error("Rate limit exceeded: {0}")]
    RateLimitError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Timeout error: {0}")]
    TimeoutError(String),

    #[error("Authentication error: {0}")]
    AuthError(String),

    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),

    #[error(transparent)]
    EnvVarError(#[from] std::env::VarError),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    #[error("XML parsing error: {0}")]
    XmlError(String),

    #[error("Agent not found: {0}")]
    AgentNotFoundError(String),

    #[error("Function execution error: {0}")]
    FunctionError(String),

    #[error("Stream processing error: {0}")]
    StreamError(String),

    #[error("Context variables error: {0}")]
    ContextError(String),

    #[error("Maximum iterations exceeded: {0}")]
    MaxIterationsError(String),

    #[error(transparent)]
    JsonError(#[from] serde_json::Error),

    #[error(transparent)]
    XmlParseError(#[from] quick_xml::DeError),

    #[error("Other error: {0}")]
    Other(String),
}

// Type alias for Result with SwarmError
pub type SwarmResult<T> = Result<T, SwarmError>;

// Convenience methods for the error type
impl SwarmError {
    pub fn is_retriable(&self) -> bool {
        matches!(
            self,
            SwarmError::NetworkError(_) |
            SwarmError::TimeoutError(_) |
            SwarmError::RateLimitError(_)
        )
    }

    pub fn is_configuration_error(&self) -> bool {
        matches!(
            self,
            SwarmError::ConfigError(_) |
            SwarmError::AuthError(_) |
            SwarmError::EnvVarError(_)
        )
    }
}

// Implement From for common error conversions
impl From<anyhow::Error> for SwarmError {
    fn from(err: anyhow::Error) -> Self {
        SwarmError::Other(err.to_string())
    }
}

impl From<std::io::Error> for SwarmError {
    fn from(err: std::io::Error) -> Self {
        SwarmError::Other(err.to_string())
    }
}