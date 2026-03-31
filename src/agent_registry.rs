//! Registry for discovering agents by capability.

use crate::types::{Agent, AgentRef};
use std::collections::HashMap;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Thread-safe registry mapping [`AgentRef`]s to [`Agent`] instances.
///
/// Used by team-formation logic to discover agents that satisfy required
/// capabilities. All operations are `&self` via an internal `RwLock`.
#[derive(Clone, Default)]
pub struct AgentRegistry {
    agents: Arc<RwLock<HashMap<AgentRef, Arc<Agent>>>>,
}

impl AgentRegistry {
    fn read_agents(&self) -> RwLockReadGuard<'_, HashMap<AgentRef, Arc<Agent>>> {
        self.agents.read().unwrap_or_else(|poisoned| {
            tracing::warn!("agent registry lock poisoned; continuing with recovered state");
            poisoned.into_inner()
        })
    }

    fn write_agents(&self) -> RwLockWriteGuard<'_, HashMap<AgentRef, Arc<Agent>>> {
        self.agents.write().unwrap_or_else(|poisoned| {
            tracing::warn!("agent registry lock poisoned; continuing with recovered state");
            poisoned.into_inner()
        })
    }

    pub fn new() -> Self {
        Default::default()
    }

    /// Register an agent, keyed by its [`AgentRef`] (derived from its name).
    ///
    /// Overwrites any previously registered agent with the same ref.
    pub fn register(&self, agent: Arc<Agent>) {
        let key = agent.agent_ref();
        self.write_agents().insert(key, agent);
    }

    /// Look up an agent by its [`AgentRef`].
    pub fn get(&self, r: &AgentRef) -> Option<Arc<Agent>> {
        self.read_agents().get(r).cloned()
    }

    /// Return all [`AgentRef`]s whose agents declare the given capability.
    pub fn find_by_capability(&self, cap: &str) -> Vec<AgentRef> {
        self.read_agents()
            .iter()
            .filter(|(_, agent)| agent.has_capability(cap))
            .map(|(r, _)| r.clone())
            .collect()
    }

    /// Return all registered [`AgentRef`]s.
    pub fn all_refs(&self) -> Vec<AgentRef> {
        self.read_agents().keys().cloned().collect()
    }

    /// Number of registered agents.
    pub fn len(&self) -> usize {
        self.read_agents().len()
    }

    pub fn is_empty(&self) -> bool {
        self.read_agents().is_empty()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Agent, Instructions};

    fn make_agent(name: &str, caps: Vec<&str>) -> Arc<Agent> {
        Arc::new(
            Agent::new(
                name,
                "gpt-4o",
                Instructions::Text(format!("{} agent", name)),
            )
            .unwrap()
            .with_capabilities(caps.into_iter().map(String::from).collect()),
        )
    }

    #[test]
    fn test_register_and_get() {
        let reg = AgentRegistry::new();
        let agent = make_agent("planner", vec!["planning"]);
        reg.register(agent.clone());

        let found = reg.get(&AgentRef::new("planner")).unwrap();
        assert_eq!(found.name(), "planner");
    }

    #[test]
    fn test_find_by_capability() {
        let reg = AgentRegistry::new();
        reg.register(make_agent("planner", vec!["planning", "reasoning"]));
        reg.register(make_agent("coder", vec!["coding", "reasoning"]));
        reg.register(make_agent("reviewer", vec!["review"]));

        let mut planners = reg.find_by_capability("planning");
        planners.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        assert_eq!(planners, vec![AgentRef::new("planner")]);

        let mut reasoners = reg.find_by_capability("reasoning");
        reasoners.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        assert_eq!(
            reasoners,
            vec![AgentRef::new("coder"), AgentRef::new("planner")]
        );

        assert!(reg.find_by_capability("nonexistent").is_empty());
    }

    #[test]
    fn test_get_missing() {
        let reg = AgentRegistry::new();
        assert!(reg.get(&AgentRef::new("nobody")).is_none());
    }
}
