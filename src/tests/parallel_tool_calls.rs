#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use serde_json::json;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::core::Swarm;
    use crate::event::{AgentEvent, EventSubscriber};
    use crate::types::{
        Agent, AgentFunction, AgentFunctionHandler, ContextVariables, Instructions, Message,
        ResultType, ToolCallExecution,
    };

    // ---------------------------------------------------------------------------
    // Test helpers
    // ---------------------------------------------------------------------------

    struct CollectingSubscriber {
        events: Mutex<Vec<AgentEvent>>,
    }

    impl CollectingSubscriber {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                events: Mutex::new(Vec::new()),
            })
        }

        fn collected(&self) -> Vec<AgentEvent> {
            self.events.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl EventSubscriber for CollectingSubscriber {
        async fn on_event(&self, event: &AgentEvent) {
            self.events.lock().unwrap().push(event.clone());
        }
    }

    fn simple_fn(name: &str, result: &str) -> AgentFunction {
        let result = result.to_string();
        let handler: Arc<AgentFunctionHandler> = Arc::new(move |_ctx: ContextVariables| {
            let r = result.clone();
            Box::pin(async move { Ok(ResultType::Value(r)) })
        });
        AgentFunction::new(name, handler, false).expect("AgentFunction::new")
    }

    fn parallel_agent(name: &str) -> Agent {
        Agent::new(
            name,
            "gpt-4",
            Instructions::Text("You are a test agent.".to_string()),
        )
        .expect("Agent::new")
        .with_functions(vec![simple_fn("tool_a", "result_a"), simple_fn("tool_b", "result_b")])
        .with_tool_call_execution(ToolCallExecution::Parallel)
    }

    fn serial_agent(name: &str) -> Agent {
        Agent::new(
            name,
            "gpt-4",
            Instructions::Text("You are a test agent.".to_string()),
        )
        .expect("Agent::new")
        .with_functions(vec![simple_fn("tool_a", "result_a"), simple_fn("tool_b", "result_b")])
        .with_tool_call_execution(ToolCallExecution::Serial)
    }

    fn two_tool_calls_response() -> serde_json::Value {
        json!({
            "id": "cmpl-multi",
            "object": "chat.completion",
            "created": 0,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "tool_calls": [
                        {"id": "c1", "type": "function",
                         "function": {"name": "tool_a", "arguments": "{}"}},
                        {"id": "c2", "type": "function",
                         "function": {"name": "tool_b", "arguments": "{}"}}
                    ]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": null
        })
    }

    // ---------------------------------------------------------------------------
    // 1. Parallel: both tools execute, both ToolCall + ToolResult events emitted
    // ---------------------------------------------------------------------------
    #[tokio::test]
    async fn test_parallel_two_tool_calls_both_executed() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(two_tool_calls_response()),
            )
            .mount(&mock_server)
            .await;

        let collector = CollectingSubscriber::new();
        let agent = parallel_agent("parallel-runner");
        let swarm = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_api_url(mock_server.uri())
            .with_agent(agent.clone())
            .with_subscriber(collector.clone())
            .build()
            .expect("swarm build");

        let response = swarm
            .run(
                agent,
                vec![Message::user("run both tools").expect("user msg")],
                ContextVariables::new(),
                None,
                false,
                false,
                5,
            )
            .await
            .expect("run should succeed");

        let events = collector.collected();
        let tool_call_count = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::ToolCall { .. }))
            .count();
        let tool_result_count = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::ToolResult { .. }))
            .count();

        assert_eq!(tool_call_count, 2, "expected 2 ToolCall events; got {}", tool_call_count);
        assert_eq!(
            tool_result_count, 2,
            "expected 2 ToolResult events; got {}",
            tool_result_count
        );

        // History contains tool result messages for both calls
        let tool_results: Vec<_> = response
            .messages
            .iter()
            .filter(|m| m.tool_call_id().is_some())
            .collect();
        assert_eq!(
            tool_results.len(),
            2,
            "expected 2 tool result messages in history; got {}",
            tool_results.len()
        );
        let contents: Vec<_> = tool_results.iter().filter_map(|m| m.content()).collect();
        assert!(contents.contains(&"result_a"), "expected result_a in tool results");
        assert!(contents.contains(&"result_b"), "expected result_b in tool results");
    }

    // ---------------------------------------------------------------------------
    // 2. Serial: same response, both tools still execute (just one-at-a-time)
    // ---------------------------------------------------------------------------
    #[tokio::test]
    async fn test_serial_two_tool_calls_both_executed() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(two_tool_calls_response()),
            )
            .mount(&mock_server)
            .await;

        let collector = CollectingSubscriber::new();
        let agent = serial_agent("serial-runner");
        let swarm = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_api_url(mock_server.uri())
            .with_agent(agent.clone())
            .with_subscriber(collector.clone())
            .build()
            .expect("swarm build");

        let response = swarm
            .run(
                agent,
                vec![Message::user("run both tools serially").expect("user msg")],
                ContextVariables::new(),
                None,
                false,
                false,
                5,
            )
            .await
            .expect("run should succeed");

        let events = collector.collected();
        let tool_call_count = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::ToolCall { .. }))
            .count();
        assert_eq!(tool_call_count, 2, "expected 2 ToolCall events; got {}", tool_call_count);

        let tool_results: Vec<_> = response
            .messages
            .iter()
            .filter(|m| m.tool_call_id().is_some())
            .collect();
        assert_eq!(
            tool_results.len(),
            2,
            "expected 2 tool result messages in history; got {}",
            tool_results.len()
        );
    }

    // ---------------------------------------------------------------------------
    // 3. Single tool_call still routes through the legacy function_call path
    //    (normalization: tc_count == 1 → promotes to function_call)
    // ---------------------------------------------------------------------------
    #[tokio::test]
    async fn test_single_tool_call_uses_legacy_path() {
        let mock_server = MockServer::start().await;

        // Single tool_call → normalization promotes it to function_call
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "cmpl-single",
                "object": "chat.completion",
                "created": 0,
                "model": "gpt-4",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "tool_calls": [{
                            "id": "c1", "type": "function",
                            "function": {"name": "tool_a", "arguments": "{}"}
                        }]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": null
            })))
            .mount(&mock_server)
            .await;

        let collector = CollectingSubscriber::new();
        let agent = parallel_agent("single-tool-agent");
        let swarm = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_api_url(mock_server.uri())
            .with_agent(agent.clone())
            .with_subscriber(collector.clone())
            .build()
            .expect("swarm build");

        let response = swarm
            .run(
                agent,
                vec![Message::user("run one tool").expect("user msg")],
                ContextVariables::new(),
                None,
                false,
                false,
                5,
            )
            .await
            .expect("single-tool run should succeed");

        let events = collector.collected();
        let tool_call_count = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::ToolCall { .. }))
            .count();
        assert_eq!(tool_call_count, 1, "expected exactly 1 ToolCall event");

        // Legacy path: result is in a function-role message (not tool_call_id)
        let has_result = response.messages.iter().any(|m| {
            m.content() == Some("result_a")
        });
        assert!(has_result, "expected 'result_a' in response messages");
    }

    // ---------------------------------------------------------------------------
    // 4. handle_tool_calls_parallel clones context so tools run independently
    // ---------------------------------------------------------------------------
    #[tokio::test]
    async fn test_context_variables_merged_after_parallel_calls() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "cmpl-ctx",
                "object": "chat.completion",
                "created": 0,
                "model": "gpt-4",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "tool_calls": [
                            {"id": "cx1", "type": "function",
                             "function": {"name": "ctx_setter_a", "arguments": "{}"}},
                            {"id": "cx2", "type": "function",
                             "function": {"name": "ctx_setter_b", "arguments": "{}"}}
                        ]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": null
            })))
            .mount(&mock_server)
            .await;

        // Two functions that each set a distinct context variable
        let fn_a: Arc<AgentFunctionHandler> = Arc::new(|_ctx: ContextVariables| {
            Box::pin(async move {
                let mut ctx = ContextVariables::new();
                ctx.insert("key_a".to_string(), "val_a".to_string());
                Ok(ResultType::ContextVariables(ctx))
            })
        });
        let fn_b: Arc<AgentFunctionHandler> = Arc::new(|_ctx: ContextVariables| {
            Box::pin(async move {
                let mut ctx = ContextVariables::new();
                ctx.insert("key_b".to_string(), "val_b".to_string());
                Ok(ResultType::ContextVariables(ctx))
            })
        });

        let agent = Agent::new(
            "ctx-agent",
            "gpt-4",
            Instructions::Text("context test".to_string()),
        )
        .expect("agent")
        .with_functions(vec![
            AgentFunction::new("ctx_setter_a", fn_a, false).expect("fn_a"),
            AgentFunction::new("ctx_setter_b", fn_b, false).expect("fn_b"),
        ])
        .with_tool_call_execution(ToolCallExecution::Parallel);

        let swarm = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_api_url(mock_server.uri())
            .with_agent(agent.clone())
            .build()
            .expect("swarm build");

        let response = swarm
            .run(
                agent,
                vec![Message::user("set context").expect("user msg")],
                ContextVariables::new(),
                None,
                false,
                false,
                5,
            )
            .await
            .expect("context vars run should succeed");

        // Both keys must be present in the returned context_variables
        assert_eq!(
            response.context_variables.get("key_a").map(String::as_str),
            Some("val_a"),
            "key_a missing from merged context"
        );
        assert_eq!(
            response.context_variables.get("key_b").map(String::as_str),
            Some("val_b"),
            "key_b missing from merged context"
        );
    }
}
