pub const CTX_VARS_NAME: &str = "context_variables";
pub const OPENAI_DEFAULT_API_URL: &str = "https://api.openai.com/v1/chat/completions";
pub const ROLE_ASSISTANT: &str = "assistant";
pub const ROLE_FUNCTION: &str = "function";
pub const ROLE_SYSTEM: &str = "system";
pub const DEFAULT_REQUEST_TIMEOUT: u64 = 30; // 30 seconds timeout
pub const DEFAULT_CONNECT_TIMEOUT: u64 = 10; // 10 seconds for connection timeout
pub const VALID_API_URL_PREFIXES: [&str; 2] = [
    "https://api.openai.com",
    "https://api.azure.com/openai",
];
pub const DEFAULT_API_VERSION: &str = "v1";
pub const DEFAULT_MAX_LOOP_ITERATIONS: u32 = 10;
pub const DEFAULT_ITERATION_DELAY_MS: u64 = 100;
pub const DEFAULT_BREAK_CONDITIONS: [&str; 1] = ["end_loop"];
pub const MIN_REQUEST_TIMEOUT: u64 = 5;
pub const MAX_REQUEST_TIMEOUT: u64 = 300;