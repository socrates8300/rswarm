// ./src/lib.rs
pub mod constants;
pub mod core;
pub mod types;
pub mod util;
pub mod validation;

pub use crate::core::Swarm;
pub use crate::types::{Agent, Instructions, Message, Response, SwarmConfig};

pub mod error;
pub use error::{SwarmError, SwarmResult};

pub mod tests;