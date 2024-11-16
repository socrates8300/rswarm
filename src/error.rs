/// Error types for the Swarm library
///
/// This module provides a comprehensive error handling system for all operations
/// within the Swarm library, including API communication, configuration,
/// validation, and agent interactions.
use thiserror::Error;

/// Main error type for the Swarm library
///
/// Encompasses all possible error conditions that can occur during
/// Swarm operations, from API communication to agent execution.
///
/// # Examples
///
/// ```rust
/// use rswarm::SwarmError;
///
/// let error = SwarmError::ApiError("Rate limit exceeded".to_string());
/// assert_eq!(error.to_string(), "API error: Rate limit exceeded");
///
/// // Checking if an error is retriable
/// assert!(SwarmError::NetworkError("Connection refused".to_string()).is_retriable());
/// ```
#[derive(Error, Debug)]
pub enum SwarmError {
    /// Errors returned by the API
    #[error("API error: {0}")]
    ApiError(String),

    /// Configuration-related errors
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Errors related to agent execution
    #[error("Agent error: {0}")]
    AgentError(String),

    /// Input validation errors
    #[error("Invalid input: {0}")]
    ValidationError(String),

    /// Rate limiting errors from the API
    #[error("Rate limit exceeded: {0}")]
    RateLimitError(String),

    /// Network communication errors
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Timeout-related errors
    #[error("Timeout error: {0}")]
    TimeoutError(String),

    /// Authentication and authorization errors
    #[error("Authentication error: {0}")]
    AuthError(String),

    /// HTTP client errors from reqwest
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),

    /// Environment variable errors
    #[error(transparent)]
    EnvVarError(#[from] std::env::VarError),

    /// Data serialization errors
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Data deserialization errors
    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    /// XML parsing and processing errors
    #[error("XML parsing error: {0}")]
    XmlError(String),

    /// Errors when an agent is not found in the registry
    #[error("Agent not found: {0}")]
    AgentNotFoundError(String),

    /// Function execution errors
    #[error("Function execution error: {0}")]
    FunctionError(String),

    /// Stream processing errors
    #[error("Stream processing error: {0}")]
    StreamError(String),

    /// Context variables related errors
    #[error("Context variables error: {0}")]
    ContextError(String),

    /// Maximum iterations exceeded errors
    #[error("Maximum iterations exceeded: {0}")]
    MaxIterationsError(String),

    /// JSON processing errors
    #[error(transparent)]
    JsonError(#[from] serde_json::Error),

    /// XML parsing errors from quick-xml
    #[error(transparent)]
    XmlParseError(#[from] quick_xml::DeError),

    /// Generic/catch-all errors
    #[error("Other error: {0}")]
    Other(String),

    /// Request timeout errors with duration
    #[error("Request timed out after {0} seconds")]
    RequestTimeoutError(u64),

    /// URL validation errors
    #[error("URL validation error: {0}")]
    UrlValidationError(String),
}

/// Type alias for Results using SwarmError
///
/// Provides a convenient way to return Results with SwarmError as the error type.
///
/// # Examples
///
/// ```rust
/// use rswarm::{SwarmResult, SwarmError};
///
/// fn example_function() -> SwarmResult<String> {
///     Ok("Success".to_string())
/// }
///
/// fn error_function() -> SwarmResult<()> {
///     Err(SwarmError::ValidationError("Invalid input".to_string()))
/// }
/// ```
pub type SwarmResult<T> = Result<T, SwarmError>;

impl SwarmError {
    /// Determines if the error is potentially retriable
    ///
    /// Returns true for errors that might succeed on retry, such as
    /// network errors, timeouts, and rate limit errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rswarm::SwarmError;
    ///
    /// let error = SwarmError::NetworkError("Connection reset".to_string());
    /// assert!(error.is_retriable());
    ///
    /// let error = SwarmError::ValidationError("Invalid input".to_string());
    /// assert!(!error.is_retriable());
    /// ```
    pub fn is_retriable(&self) -> bool {
        matches!(
            self,
            SwarmError::NetworkError(_) |
            SwarmError::TimeoutError(_) |
            SwarmError::RateLimitError(_)
        )
    }

    /// Determines if the error is related to configuration
    ///
    /// Returns true for errors that indicate configuration problems,
    /// such as invalid settings or missing credentials.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rswarm::SwarmError;
    ///
    /// let error = SwarmError::ConfigError("Invalid API URL".to_string());
    /// assert!(error.is_configuration_error());
    ///
    /// let error = SwarmError::NetworkError("Connection failed".to_string());
    /// assert!(!error.is_configuration_error());
    /// ```
    pub fn is_configuration_error(&self) -> bool {
        matches!(
            self,
            SwarmError::ConfigError(_) |
            SwarmError::AuthError(_) |
            SwarmError::EnvVarError(_)
        )
    }
}

/// Implement From for common error conversions
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