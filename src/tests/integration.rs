use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::json;

use crate::core::Swarm;
use crate::error::SwarmError;
use crate::event::{AgentEvent, EventSubscriber};
use crate::tool::{ClosureTool, InvocationArgs, Tool};
use crate::types::{AgentFunction, ContextVariables, Instructions, Message, ResultType};

// --- Test subscriber that collects events -----------------------------------

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

// --- Helpers ----------------------------------------------------------------

fn chat_response_body(content: &str) -> String {
    json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "created": 0,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content,
                "name": null,
                "function_call": null
            },
            "finish_reason": "stop"
        }],
        "usage": null
    })
    .to_string()
}

// --- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Agent;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn text_agent(name: &str, instructions: &str) -> Agent {
        Agent::new(name, "gpt-4", Instructions::Text(instructions.to_string()))
            .expect("agent creation failed")
    }

    // 1. ClosureTool wraps an AgentFunction and executes it via the Tool trait
    #[tokio::test]
    async fn test_closure_tool_executes() {
        use std::future::Future;
        use std::pin::Pin;

        let fn_arc: Arc<
            dyn Fn(
                    ContextVariables,
                )
                    -> Pin<Box<dyn Future<Output = Result<ResultType, anyhow::Error>> + Send>>
                + Send
                + Sync,
        > = Arc::new(|_ctx: ContextVariables| {
            Box::pin(async move { Ok(ResultType::Value("hello from closure".to_string())) })
        });

        let agent_fn = AgentFunction {
            name: "greet".to_string(),
            function: fn_arc,
            accepts_context_variables: false,
        };

        let tool =
            ClosureTool::from_agent_function(agent_fn).with_description("greet the user");

        assert_eq!(tool.name(), "greet");
        assert_eq!(tool.description(), "greet the user");

        let args = InvocationArgs::from_value(json!({})).unwrap();
        let result = tool.execute(args).await.expect("execute failed");
        assert_eq!(result, json!("hello from closure"));
    }

    // 2. MaxIterationsError carries structured max/actual fields
    #[test]
    fn test_max_iterations_error_fields() {
        let err = SwarmError::MaxIterationsError { max: 5, actual: 7 };
        let msg = err.to_string();
        assert!(msg.contains('5'), "expected max in message: {}", msg);
        assert!(msg.contains('7'), "expected actual in message: {}", msg);
        match err {
            SwarmError::MaxIterationsError { max, actual } => {
                assert_eq!(max, 5);
                assert_eq!(actual, 7);
            }
            _ => panic!("wrong variant"),
        }
    }

    // 3. LoopStart, LlmRequest, LlmResponse, and LoopEnd events are emitted
    #[tokio::test]
    async fn test_loop_events_emitted() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({
                    "id": "chatcmpl-test",
                    "object": "chat.completion",
                    "created": 0,
                    "model": "gpt-4",
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": "done"
                        },
                        "finish_reason": "stop"
                    }],
                    "usage": null
                })),
            )
            .mount(&mock_server)
            .await;

        let collector = CollectingSubscriber::new();
        let agent = text_agent("helper", "You are a helpful assistant.");

        let swarm = Swarm::builder()
            .with_api_key("sk-test".to_string())
            .with_api_url(mock_server.uri())
            .with_agent(agent.clone())
            .with_subscriber(collector.clone())
            .build()
            .expect("build failed");

        let messages = vec![Message::user("hi").expect("message failed")];
        swarm
            .run(agent, messages, ContextVariables::new(), None, false, false, 5)
            .await
            .expect("run failed");

        let events = collector.collected();
        assert!(
            events.iter().any(|e| matches!(e, AgentEvent::LoopStart { .. })),
            "LoopStart not emitted; got: {:?}",
            events.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
        assert!(
            events.iter().any(|e| matches!(e, AgentEvent::LlmRequest { .. })),
            "LlmRequest not emitted"
        );
        assert!(
            events.iter().any(|e| matches!(e, AgentEvent::LlmResponse { .. })),
            "LlmResponse not emitted"
        );
        assert!(
            events.iter().any(|e| matches!(e, AgentEvent::LoopEnd { .. })),
            "LoopEnd not emitted"
        );
    }

    // 4. OpenAiProvider::complete() POSTs a valid request and parses CompletionResponse
    #[tokio::test]
    async fn test_open_ai_provider_complete() {
        use crate::provider::{CompletionRequest, LlmProvider, OpenAiProvider};

        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(
                    json!({
                        "id": "cmp-1",
                        "object": "chat.completion",
                        "created": 0,
                        "model": "gpt-4",
                        "choices": [{
                            "index": 0,
                            "message": { "role": "assistant", "content": "pong" },
                            "finish_reason": "stop"
                        }],
                        "usage": null
                    })
                    .to_string(),
                    "application/json",
                ),
            )
            .mount(&mock_server)
            .await;

        let provider = OpenAiProvider::new(reqwest::Client::new(), "sk-test", mock_server.uri());

        let req = CompletionRequest::new("gpt-4", vec![Message::user("ping").expect("msg")]);
        let resp = provider.complete(req).await.expect("complete failed");

        assert_eq!(resp.text().as_deref(), Some("pong"));
        assert_eq!(resp.model, "gpt-4");
    }
}
