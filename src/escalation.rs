//! Heuristic escalation triggers for runaway and degenerate agent loops (task #46).
//!
//! An [`EscalationDetector`] is embedded in the run loop and updated after
//! every tool call and LLM response. When a trigger fires, the configured
//! [`EscalationAction`] is returned to the caller so the loop can respond
//! appropriately without hard-coding the response policy.
//!
//! # Triggers
//!
//! | Trigger | What it detects |
//! |---------|-----------------|
//! | [`EscalationTrigger::RepeatedFailure`] | Same tool failing N consecutive times |
//! | [`EscalationTrigger::HallucinatedTool`] | LLM requests a tool that is not registered |
//! | [`EscalationTrigger::LoopDetected`] | Identical (tool, args) pair repeated within the window |
//!
//! # Usage
//!
//! ```rust,ignore
//! let config = EscalationConfig::default();
//! let mut detector = EscalationDetector::new(config);
//!
//! // After each tool call:
//! if let Some(trigger) = detector.record_tool_call("my_tool", false, &known_tools) {
//!     match detector.config().action {
//!         EscalationAction::Stop => { /* terminate */ }
//!         EscalationAction::InjectWarning(msg) => { /* inject msg into history */ }
//!         EscalationAction::HumanReviewEvent => { /* emit event */ }
//!     }
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Trigger types
// ---------------------------------------------------------------------------

/// The kind of problematic pattern that was detected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalationTrigger {
    /// The same tool has failed `n` consecutive times.
    RepeatedFailure { tool: String, consecutive: u32 },
    /// The LLM requested a tool that is not in the registry.
    HallucinatedTool { tool: String },
    /// The same (tool, argument signature) pair has appeared within the
    /// recent window, indicating a circular reasoning pattern.
    LoopDetected { tool: String, occurrences: u32 },
}

impl std::fmt::Display for EscalationTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RepeatedFailure { tool, consecutive } => {
                write!(f, "repeated_failure(tool={}, n={})", tool, consecutive)
            }
            Self::HallucinatedTool { tool } => write!(f, "hallucinated_tool({})", tool),
            Self::LoopDetected { tool, occurrences } => {
                write!(f, "loop_detected(tool={}, n={})", tool, occurrences)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Escalation action
// ---------------------------------------------------------------------------

/// What the loop should do when escalation fires.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalationAction {
    /// Terminate the loop immediately.
    Stop,
    /// Inject a warning message into the conversation history and continue.
    InjectWarning,
    /// Emit a structured event requesting human review and continue.
    HumanReviewEvent,
}

// ---------------------------------------------------------------------------
// EscalationConfig
// ---------------------------------------------------------------------------

/// Thresholds and response policy for the escalation detector.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EscalationConfig {
    /// Number of consecutive failures of the same tool before triggering.
    pub repeated_failure_threshold: u32,
    /// How many recent (tool, signature) pairs to remember for loop detection.
    pub loop_detection_window: usize,
    /// How many times the same (tool, signature) can appear in the window.
    pub loop_occurrence_threshold: u32,
    /// Action to take when any trigger fires.
    pub action: EscalationAction,
}

impl Default for EscalationConfig {
    fn default() -> Self {
        Self {
            repeated_failure_threshold: 3,
            loop_detection_window: 10,
            loop_occurrence_threshold: 3,
            action: EscalationAction::InjectWarning,
        }
    }
}

// ---------------------------------------------------------------------------
// EscalationDetector
// ---------------------------------------------------------------------------

/// Stateful detector that tracks tool call history and fires triggers.
pub struct EscalationDetector {
    config: EscalationConfig,
    /// Consecutive failure count per tool name.
    consecutive_failures: HashMap<String, u32>,
    /// Ring-buffer of recent (tool, arg_signature) pairs.
    recent_calls: Vec<(String, String)>,
}

impl EscalationDetector {
    pub fn new(config: EscalationConfig) -> Self {
        Self {
            config,
            consecutive_failures: HashMap::new(),
            recent_calls: Vec::new(),
        }
    }

    pub fn config(&self) -> &EscalationConfig {
        &self.config
    }

    /// Record a tool call outcome and check for escalation triggers.
    ///
    /// - `tool_name`: the tool that was invoked.
    /// - `success`: whether the call succeeded.
    /// - `known_tools`: the set of registered tool names (for hallucination check).
    /// - `arg_signature`: a stable string representation of the call arguments
    ///   (e.g. `serde_json::to_string(&args).unwrap_or_default()`).
    ///
    /// Returns the first trigger that fires, or `None` if all is well.
    pub fn record_tool_call(
        &mut self,
        tool_name: &str,
        success: bool,
        known_tools: &[&str],
        arg_signature: &str,
    ) -> Option<EscalationTrigger> {
        // Check hallucination first (before touching failure counts).
        if !known_tools.contains(&tool_name) {
            return Some(EscalationTrigger::HallucinatedTool {
                tool: tool_name.to_string(),
            });
        }

        // Update consecutive failure counter.
        let failures = self
            .consecutive_failures
            .entry(tool_name.to_string())
            .or_insert(0);
        if success {
            *failures = 0;
        } else {
            *failures += 1;
            if *failures >= self.config.repeated_failure_threshold {
                let n = *failures;
                return Some(EscalationTrigger::RepeatedFailure {
                    tool: tool_name.to_string(),
                    consecutive: n,
                });
            }
        }

        // Update the recent-call ring buffer and check for loops.
        let key = (tool_name.to_string(), arg_signature.to_string());
        self.recent_calls.push(key.clone());
        if self.recent_calls.len() > self.config.loop_detection_window {
            self.recent_calls.remove(0);
        }
        let occurrences = self.recent_calls.iter().filter(|e| *e == &key).count() as u32;
        if occurrences >= self.config.loop_occurrence_threshold {
            return Some(EscalationTrigger::LoopDetected {
                tool: tool_name.to_string(),
                occurrences,
            });
        }

        None
    }

    /// Reset all state (call when starting a fresh session).
    pub fn reset(&mut self) {
        self.consecutive_failures.clear();
        self.recent_calls.clear();
    }

    /// Generate the warning message to inject into the agent's history.
    pub fn warning_message(trigger: &EscalationTrigger) -> String {
        match trigger {
            EscalationTrigger::RepeatedFailure { tool, consecutive } => format!(
                "[SYSTEM WARNING] Tool '{}' has failed {} consecutive times. \
                 Consider an alternative approach or stopping.",
                tool, consecutive
            ),
            EscalationTrigger::HallucinatedTool { tool } => format!(
                "[SYSTEM WARNING] Tool '{}' does not exist. \
                 Please use only the tools that are available to you.",
                tool
            ),
            EscalationTrigger::LoopDetected { tool, occurrences } => format!(
                "[SYSTEM WARNING] Possible circular reasoning detected: \
                 tool '{}' has been called with identical arguments {} times recently. \
                 Try a different approach.",
                tool, occurrences
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const TOOLS: &[&str] = &["search", "calculator", "file_read"];

    #[test]
    fn test_no_trigger_on_normal_calls() {
        let mut d = EscalationDetector::new(EscalationConfig::default());
        assert!(d
            .record_tool_call("search", true, TOOLS, r#"{"q":"hello"}"#)
            .is_none());
        assert!(d
            .record_tool_call("calculator", true, TOOLS, r#"{"expr":"1+1"}"#)
            .is_none());
    }

    #[test]
    fn test_hallucinated_tool() {
        let mut d = EscalationDetector::new(EscalationConfig::default());
        let trigger = d.record_tool_call("nonexistent_tool", true, TOOLS, "{}");
        assert!(matches!(
            trigger,
            Some(EscalationTrigger::HallucinatedTool { .. })
        ));
    }

    #[test]
    fn test_repeated_failure_trigger() {
        let config = EscalationConfig {
            repeated_failure_threshold: 2,
            ..EscalationConfig::default()
        };
        let mut d = EscalationDetector::new(config);
        assert!(d.record_tool_call("search", false, TOOLS, "{}").is_none()); // 1
        let trigger = d.record_tool_call("search", false, TOOLS, "{}"); // 2 → trigger
        assert!(matches!(
            trigger,
            Some(EscalationTrigger::RepeatedFailure { consecutive: 2, .. })
        ));
    }

    #[test]
    fn test_success_resets_failure_count() {
        let config = EscalationConfig {
            repeated_failure_threshold: 2,
            ..EscalationConfig::default()
        };
        let mut d = EscalationDetector::new(config);
        d.record_tool_call("search", false, TOOLS, "{}");
        d.record_tool_call("search", true, TOOLS, "{}"); // reset
        d.record_tool_call("search", false, TOOLS, "{}"); // 1 again
        let trigger = d.record_tool_call("search", false, TOOLS, "{}"); // 2 → trigger
        assert!(matches!(
            trigger,
            Some(EscalationTrigger::RepeatedFailure { consecutive: 2, .. })
        ));
    }

    #[test]
    fn test_loop_detection() {
        let config = EscalationConfig {
            loop_occurrence_threshold: 2,
            loop_detection_window: 5,
            ..EscalationConfig::default()
        };
        let mut d = EscalationDetector::new(config);
        let args = r#"{"q":"same"}"#;
        assert!(d.record_tool_call("search", true, TOOLS, args).is_none()); // 1
        let trigger = d.record_tool_call("search", true, TOOLS, args); // 2 → trigger
        assert!(matches!(
            trigger,
            Some(EscalationTrigger::LoopDetected { occurrences: 2, .. })
        ));
    }

    #[test]
    fn test_different_args_no_loop() {
        let config = EscalationConfig {
            loop_occurrence_threshold: 2,
            ..EscalationConfig::default()
        };
        let mut d = EscalationDetector::new(config);
        d.record_tool_call("search", true, TOOLS, r#"{"q":"a"}"#);
        let trigger = d.record_tool_call("search", true, TOOLS, r#"{"q":"b"}"#);
        assert!(trigger.is_none()); // different args → no loop
    }

    #[test]
    fn test_warning_messages_non_empty() {
        let triggers = vec![
            EscalationTrigger::HallucinatedTool { tool: "x".into() },
            EscalationTrigger::RepeatedFailure {
                tool: "x".into(),
                consecutive: 3,
            },
            EscalationTrigger::LoopDetected {
                tool: "x".into(),
                occurrences: 3,
            },
        ];
        for t in &triggers {
            assert!(!EscalationDetector::warning_message(t).is_empty());
        }
    }
}
