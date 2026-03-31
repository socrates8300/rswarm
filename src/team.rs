//! Team-formation and coordination primitives for multi-agent workloads.

use crate::error::{SwarmError, SwarmResult};
use crate::types::AgentRef;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamRole {
    name: String,
    required_capabilities: Vec<String>,
    optional_capabilities: Vec<String>,
}

impl TeamRole {
    pub fn new(name: impl Into<String>, required_capabilities: Vec<String>) -> SwarmResult<Self> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "TeamRole name cannot be empty".to_string(),
            ));
        }
        if required_capabilities
            .iter()
            .any(|capability| capability.trim().is_empty())
        {
            return Err(SwarmError::ValidationError(
                "TeamRole required capabilities cannot contain empty values".to_string(),
            ));
        }
        Ok(Self {
            name,
            required_capabilities,
            optional_capabilities: Vec::new(),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn required_capabilities(&self) -> &[String] {
        &self.required_capabilities
    }

    pub fn optional_capabilities(&self) -> &[String] {
        &self.optional_capabilities
    }

    pub fn with_optional_capabilities(
        mut self,
        optional_capabilities: Vec<String>,
    ) -> SwarmResult<Self> {
        if optional_capabilities
            .iter()
            .any(|capability| capability.trim().is_empty())
        {
            return Err(SwarmError::ValidationError(
                "TeamRole optional capabilities cannot contain empty values".to_string(),
            ));
        }
        self.optional_capabilities = optional_capabilities;
        Ok(self)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamFormationPolicy {
    pub allow_agent_reuse: bool,
    pub prefer_existing_assignments: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamAssignment {
    role: TeamRole,
    agent: AgentRef,
}

impl TeamAssignment {
    pub fn new(role: TeamRole, agent: AgentRef) -> Self {
        Self { role, agent }
    }

    pub fn role(&self) -> &TeamRole {
        &self.role
    }

    pub fn agent(&self) -> &AgentRef {
        &self.agent
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTeam {
    assignments: Vec<TeamAssignment>,
}

impl AgentTeam {
    pub fn new(assignments: Vec<TeamAssignment>) -> SwarmResult<Self> {
        let mut seen_roles = HashSet::new();
        for assignment in &assignments {
            if !seen_roles.insert(assignment.role().name().to_string()) {
                return Err(SwarmError::ValidationError(format!(
                    "Duplicate team role '{}'",
                    assignment.role().name()
                )));
            }
        }
        Ok(Self { assignments })
    }

    pub fn assignments(&self) -> &[TeamAssignment] {
        &self.assignments
    }

    pub fn agent_for_role(&self, role_name: &str) -> Option<&AgentRef> {
        self.assignments
            .iter()
            .find(|assignment| assignment.role().name() == role_name)
            .map(TeamAssignment::agent)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsensusStrategy {
    Majority,
    Unanimous,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamVote {
    agent: AgentRef,
    option: String,
    weight: u32,
}

impl TeamVote {
    pub fn new(agent: AgentRef, option: impl Into<String>) -> SwarmResult<Self> {
        let option = option.into();
        if option.trim().is_empty() {
            return Err(SwarmError::ValidationError(
                "Consensus option cannot be empty".to_string(),
            ));
        }
        Ok(Self {
            agent,
            option,
            weight: 1,
        })
    }

    pub fn with_weight(mut self, weight: u32) -> SwarmResult<Self> {
        if weight == 0 {
            return Err(SwarmError::ValidationError(
                "Consensus vote weight must be greater than 0".to_string(),
            ));
        }
        self.weight = weight;
        Ok(self)
    }

    pub fn agent(&self) -> &AgentRef {
        &self.agent
    }

    pub fn option(&self) -> &str {
        &self.option
    }

    pub fn weight(&self) -> u32 {
        self.weight
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoteTally {
    option: String,
    weight: u32,
}

impl VoteTally {
    pub(crate) fn new(option: String, weight: u32) -> Self {
        Self { option, weight }
    }

    pub fn option(&self) -> &str {
        &self.option
    }

    pub fn weight(&self) -> u32 {
        self.weight
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamDecision {
    strategy: ConsensusStrategy,
    selected_option: String,
    total_votes: u32,
    tallies: Vec<VoteTally>,
    unanimous: bool,
}

impl TeamDecision {
    pub(crate) fn new(
        strategy: ConsensusStrategy,
        selected_option: String,
        total_votes: u32,
        tallies: Vec<VoteTally>,
        unanimous: bool,
    ) -> Self {
        Self {
            strategy,
            selected_option,
            total_votes,
            tallies,
            unanimous,
        }
    }

    pub fn strategy(&self) -> ConsensusStrategy {
        self.strategy
    }

    pub fn selected_option(&self) -> &str {
        &self.selected_option
    }

    pub fn total_votes(&self) -> u32 {
        self.total_votes
    }

    pub fn tallies(&self) -> &[VoteTally] {
        &self.tallies
    }

    pub fn unanimous(&self) -> bool {
        self.unanimous
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_team_role_rejects_empty_name() {
        let error = TeamRole::new("", vec!["planning".to_string()]).expect_err("empty role");
        assert!(error.to_string().contains("TeamRole name cannot be empty"));
    }

    #[test]
    fn test_team_rejects_duplicate_role_names() {
        let planner = TeamRole::new("planner", vec!["planning".to_string()]).expect("role");
        let assignments = vec![
            TeamAssignment::new(planner.clone(), AgentRef::new("alice")),
            TeamAssignment::new(planner, AgentRef::new("bob")),
        ];
        let error = AgentTeam::new(assignments).expect_err("duplicate role");
        assert!(error.to_string().contains("Duplicate team role"));
    }

    #[test]
    fn test_team_vote_rejects_zero_weight() {
        let error = TeamVote::new(AgentRef::new("alice"), "approve")
            .expect("vote")
            .with_weight(0)
            .expect_err("zero weight");
        assert!(error.to_string().contains("weight must be greater than 0"));
    }
}
