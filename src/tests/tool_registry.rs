//! Integration tests for `ToolRegistry` dispatch in `Swarm::run`.
//!
//! These tests cover Phase 2a of the tool-abstraction migration: a `Tool`
//! implementation registered on the `Swarm` is advertised to the LLM through
//! the OpenAI tools wire format and dispatched when the model issues a
//! `tool_call` for it. The tests also exercise the marker conventions
//! (`__rswarm_agent_handoff`, `__rswarm_context_update`, `__rswarm_termination`)
//! that let a `Tool` express non-`Value` `ResultType` variants.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use serde_json::{json, Value};
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::core::Swarm;
    use crate::phase::TerminationReason;
    use crate::tool::{InvocationArgs, Tool, ToolError, ToolRegistry};
    use crate::types::{Agent, Instructions, Message};
    use crate::RunOptions;

    /// A minimal `Tool` that records the args it was called with and returns
    /// a configurable JSON value.
    struct EchoTool {
        name: String,
        description: String,
        return_value: Value,
        recorded_args: Arc<Mutex<Vec<Value>>>,
    }

    impl EchoTool {
        fn new(name: &str, description: &str, return_value: Value) -> Self {
            Self {
                name: name.to_string(),
                description: description.to_string(),
                return_value,
                recorded_args: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn calls(&self) -> Arc<Mutex<Vec<Value>>> {
            Arc::clone(&self.recorded_args)
        }
    }

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            &self.description
        }

        fn parameters_schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                },
                "required": []
            })
        }

        async fn execute(&self, args: InvocationArgs) -> Result<Value, ToolError> {
            self.recorded_args
                .lock()
                .unwrap()
                .push(args.as_value().clone());
            Ok(self.return_value.clone())
        }
    }

    fn agent(name: &str) -> Agent {
        Agent::new(
            name,
            "gpt-4",
            Instructions::Text("registry-dispatch test".to_string()),
        )
        .expect("Agent::new")
    }

    fn one_tool_call_response(tool_name: &str, args: &str) -> Value {
        json!({
            "id": "cmpl-tool-registry",
            "object": "chat.completion",
            "created": 0,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "tc-1",
                        "type": "function",
                        "function": { "name": tool_name, "arguments": args }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": null
        })
    }

    // -----------------------------------------------------------------------
    // 1. Registered Tool dispatches and returns its value
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_registered_tool_is_dispatched() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(one_tool_call_response("lookup", r#"{"query":"hello"}"#)),
            )
            .mount(&mock_server)
            .await;

        let tool = EchoTool::new("lookup", "Look up a value", Value::String("found".into()));
        let calls = tool.calls();

        let mut registry = ToolRegistry::new();
        registry.register(tool);

        let agent = agent("registry-runner");
        let swarm = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_api_url(mock_server.uri())
            .with_agent(agent.clone())
            .with_tool_registry(registry)
            .build()
            .expect("swarm build");

        let response = swarm
            .run(
                agent,
                vec![Message::user("hi").expect("user msg")],
                RunOptions {
                    max_turns: 5,
                    ..RunOptions::default()
                },
            )
            .await
            .expect("run should succeed");

        let recorded = calls.lock().unwrap().clone();
        assert_eq!(recorded.len(), 1, "tool should be invoked exactly once");
        assert_eq!(
            recorded[0],
            json!({"query":"hello"}),
            "tool should receive the LLM's arguments verbatim"
        );

        let tool_result = response
            .messages
            .iter()
            .find(|m| m.tool_call_id().is_some() || m.content() == Some("found"));
        assert!(
            tool_result.is_some(),
            "tool's return value should appear in run history"
        );
    }

    // -----------------------------------------------------------------------
    // 2. Registry takes precedence over agent.functions for same-name tools
    // -----------------------------------------------------------------------
    #[tokio::test]
    #[allow(deprecated)]
    async fn test_registry_shadows_same_name_agent_function() {
        use crate::types::{AgentFunction, AgentFunctionHandler, ContextVariables, ResultType};

        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(one_tool_call_response("describe", r#"{}"#)),
            )
            .mount(&mock_server)
            .await;

        // AgentFunction with the same name; if dispatched it returns "from_agent_fn".
        let handler: Arc<AgentFunctionHandler> = Arc::new(|_ctx: ContextVariables| {
            Box::pin(async move { Ok(ResultType::Value("from_agent_fn".into())) })
        });
        let agent_fn = AgentFunction::new("describe", handler, false).expect("AgentFunction::new");

        let tool = EchoTool::new(
            "describe",
            "Describe via registry",
            Value::String("from_registry".into()),
        );
        let calls = tool.calls();

        let mut registry = ToolRegistry::new();
        registry.register(tool);

        let agent = agent("collision-agent").with_functions(vec![agent_fn]);
        let swarm = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_api_url(mock_server.uri())
            .with_agent(agent.clone())
            .with_tool_registry(registry)
            .build()
            .expect("swarm build");

        let response = swarm
            .run(
                agent,
                vec![Message::user("describe").expect("user msg")],
                RunOptions {
                    max_turns: 5,
                    ..RunOptions::default()
                },
            )
            .await
            .expect("run should succeed");

        assert_eq!(
            calls.lock().unwrap().len(),
            1,
            "registry tool should be the one dispatched"
        );
        let has_registry_result = response
            .messages
            .iter()
            .any(|m| m.content() == Some("from_registry"));
        let has_agent_fn_result = response
            .messages
            .iter()
            .any(|m| m.content() == Some("from_agent_fn"));
        assert!(
            has_registry_result,
            "expected registry tool's value in run history"
        );
        assert!(
            !has_agent_fn_result,
            "agent_fn must not run when registry has the same name"
        );
    }

    // -----------------------------------------------------------------------
    // 3. Marker: agent handoff
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_marker_agent_handoff_switches_active_agent() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(one_tool_call_response("go_to_b", r#"{}"#)),
            )
            .mount(&mock_server)
            .await;

        let tool = EchoTool::new(
            "go_to_b",
            "Switch active agent to agent_b",
            json!({ "__rswarm_agent_handoff": "agent_b" }),
        );
        let mut registry = ToolRegistry::new();
        registry.register(tool);

        let agent_a = agent("agent_a");
        let agent_b = agent("agent_b");
        let swarm = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_api_url(mock_server.uri())
            .with_agent(agent_a.clone())
            .with_agent(agent_b)
            .with_tool_registry(registry)
            .build()
            .expect("swarm build");

        let response = swarm
            .run(
                agent_a,
                vec![Message::user("handoff").expect("user msg")],
                RunOptions {
                    max_turns: 5,
                    ..RunOptions::default()
                },
            )
            .await
            .expect("run should succeed");

        assert_eq!(
            response.agent.as_ref().map(|a| a.name()),
            Some("agent_b"),
            "active agent on the response should be agent_b after handoff"
        );
    }

    // -----------------------------------------------------------------------
    // 4. Marker: context update
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_marker_context_update_merges_into_run_context() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(one_tool_call_response("set_ctx", r#"{}"#)),
            )
            .mount(&mock_server)
            .await;

        let tool = EchoTool::new(
            "set_ctx",
            "Set context variables",
            json!({ "__rswarm_context_update": { "user_id": "42", "tier": "pro" } }),
        );
        let mut registry = ToolRegistry::new();
        registry.register(tool);

        let agent = agent("ctx-agent");
        let swarm = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_api_url(mock_server.uri())
            .with_agent(agent.clone())
            .with_tool_registry(registry)
            .build()
            .expect("swarm build");

        let response = swarm
            .run(
                agent,
                vec![Message::user("set ctx").expect("user msg")],
                RunOptions {
                    max_turns: 5,
                    ..RunOptions::default()
                },
            )
            .await
            .expect("run should succeed");

        assert_eq!(
            response
                .context_variables
                .get("user_id")
                .map(String::as_str),
            Some("42")
        );
        assert_eq!(
            response.context_variables.get("tier").map(String::as_str),
            Some("pro")
        );
    }

    // -----------------------------------------------------------------------
    // 5. Marker: termination
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_marker_termination_ends_run() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(one_tool_call_response("stop_run", r#"{}"#)),
            )
            .mount(&mock_server)
            .await;

        let tool = EchoTool::new(
            "stop_run",
            "Terminate the run",
            json!({ "__rswarm_termination": "explicit_stop" }),
        );
        let mut registry = ToolRegistry::new();
        registry.register(tool);

        let agent = agent("term-agent");
        let swarm = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_api_url(mock_server.uri())
            .with_agent(agent.clone())
            .with_tool_registry(registry)
            .build()
            .expect("swarm build");

        let response = swarm
            .run(
                agent,
                vec![Message::user("end").expect("user msg")],
                RunOptions {
                    max_turns: 5,
                    ..RunOptions::default()
                },
            )
            .await
            .expect("run should succeed");

        assert_eq!(
            response.termination_reason,
            Some(TerminationReason::ExplicitStop),
            "termination marker should set termination_reason"
        );
    }

    // -----------------------------------------------------------------------
    // 6. Unknown handoff target produces a validation error
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_handoff_to_unknown_agent_errors() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(one_tool_call_response("bad_handoff", r#"{}"#)),
            )
            .mount(&mock_server)
            .await;

        let tool = EchoTool::new(
            "bad_handoff",
            "Hand off to a non-existent agent",
            json!({ "__rswarm_agent_handoff": "ghost" }),
        );
        let mut registry = ToolRegistry::new();
        registry.register(tool);

        let agent = agent("a");
        let swarm = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_api_url(mock_server.uri())
            .with_agent(agent.clone())
            .with_tool_registry(registry)
            .build()
            .expect("swarm build");

        let result = swarm
            .run(
                agent,
                vec![Message::user("go").expect("user msg")],
                RunOptions {
                    max_turns: 3,
                    ..RunOptions::default()
                },
            )
            .await;

        match result {
            Err(err) => {
                let msg = err.to_string();
                assert!(
                    msg.contains("ghost"),
                    "error message should mention the unknown agent name; got: {}",
                    msg
                );
            }
            Ok(_) => panic!("expected validation error for unknown handoff target"),
        }
    }
}
