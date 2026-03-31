#[cfg(test)]
mod tests {
    use crate::types::{FunctionCall, MessageRole};
    use crate::util::merge_chunk_message;
    use crate::validation::validate_api_request;
    use crate::{Agent, Instructions, Message, SwarmError};
    use serde_json::json;

    fn test_agent() -> Agent {
        Agent::new(
            "message_test_agent",
            "gpt-4",
            Instructions::Text("Validate message shapes".to_string()),
        )
        .expect("Failed to create test agent")
    }

    #[test]
    fn test_message_constructors_cover_valid_roles() {
        let system = Message::system("System prompt").expect("Expected valid system message");
        let user = Message::user("Hello").expect("Expected valid user message");
        let assistant = Message::assistant("Hi").expect("Expected valid assistant message");
        let function =
            Message::function("lookup_docs", "{\"ok\":true}").expect("Expected function message");
        let function_call = FunctionCall::new("lookup_docs", "{\"query\":\"rust\"}")
            .expect("Expected valid function call");
        let assistant_call = Message::assistant_function_call(function_call.clone())
            .expect("Expected valid assistant function-call message");

        assert_eq!(system.role(), MessageRole::System);
        assert_eq!(system.content(), Some("System prompt"));
        assert_eq!(user.role(), MessageRole::User);
        assert_eq!(assistant.role(), MessageRole::Assistant);
        assert_eq!(assistant.content(), Some("Hi"));
        assert_eq!(function.role(), MessageRole::Function);
        assert_eq!(function.name(), Some("lookup_docs"));
        assert_eq!(assistant_call.function_call(), Some(&function_call));
    }

    #[test]
    fn test_message_deserialization_rejects_invalid_shapes() {
        let double_payload = serde_json::from_value::<Message>(json!({
            "role": "assistant",
            "content": "hello",
            "function_call": {
                "name": "lookup_docs",
                "arguments": "{}"
            }
        }))
        .expect_err("Assistant messages cannot carry content and function_call");
        assert!(
            double_payload.to_string().contains("exactly one of")
                || double_payload
                    .to_string()
                    .contains("either content or a function call")
        );

        let invalid_role = serde_json::from_value::<Message>(json!({
            "role": "moderator",
            "content": "hello"
        }))
        .expect_err("Unknown roles should fail deserialization");
        assert!(invalid_role.to_string().contains("unknown variant"));

        let invalid_function_call = FunctionCall::new("lookup_docs", "not-json")
            .expect_err("Function calls require JSON arguments");
        assert!(matches!(
            invalid_function_call,
            SwarmError::ValidationError(_)
        ));
    }

    #[test]
    fn test_validate_api_request_rejects_empty_history() {
        let agent = test_agent();
        let error = validate_api_request(&agent, &[], &None, 1)
            .expect_err("empty history should fail preflight validation");
        assert!(matches!(error, SwarmError::ValidationError(_)));
        assert!(error.to_string().to_lowercase().contains("empty"));
    }

    #[test]
    fn test_validate_api_request_rejects_structurally_invalid_messages() {
        let agent = test_agent();
        let invalid_empty_assistant =
            Message::from_parts_unchecked(MessageRole::Assistant, None, None, None);
        let invalid_function_without_name = Message::from_parts_unchecked(
            MessageRole::Function,
            Some("done".to_string()),
            None,
            None,
        );
        let invalid_system_function_call = Message::from_parts_unchecked(
            MessageRole::System,
            Some("system".to_string()),
            None,
            Some(FunctionCall::from_parts_unchecked(
                "lookup_docs".to_string(),
                "{}".to_string(),
            )),
        );

        for message in [
            invalid_empty_assistant,
            invalid_function_without_name,
            invalid_system_function_call,
        ] {
            let error = validate_api_request(&agent, &[message], &None, 1)
                .expect_err("Invalid message should fail request validation");
            assert!(matches!(error, SwarmError::ValidationError(_)));
        }
    }

    // --- ToolCall / MessageRole::Tool tests ------------------------------------

    #[test]
    fn test_tool_result_message_valid_and_serializes_correctly() {
        let msg = Message::tool_result("call_abc123", "42").expect("tool_result should be valid");
        assert_eq!(msg.role(), MessageRole::Tool);
        assert_eq!(msg.content(), Some("42"));
        assert_eq!(msg.tool_call_id(), Some("call_abc123"));
        assert!(msg.function_call().is_none());
        assert!(msg.tool_calls().is_none());

        let json = serde_json::to_value(&msg).expect("serialize");
        assert_eq!(json["role"], "tool");
        assert_eq!(json["content"], "42");
        assert_eq!(json["tool_call_id"], "call_abc123");
        assert!(json.get("function_call").is_none() || json["function_call"].is_null());
    }

    #[test]
    fn test_assistant_tool_calls_message_valid() {
        use crate::types::{FunctionCall, ToolCall};
        let fc = FunctionCall::new("my_tool", "{\"x\":1}").expect("fc");
        let tc = ToolCall::new("call_xyz", fc).expect("tc");
        let msg =
            Message::assistant_tool_calls(vec![tc]).expect("assistant_tool_calls should be valid");
        assert_eq!(msg.role(), MessageRole::Assistant);
        assert!(msg.content().is_none());
        assert!(msg.function_call().is_none());
        let calls = msg.tool_calls().expect("should have tool_calls");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id(), "call_xyz");
        assert_eq!(calls[0].function().name(), "my_tool");
    }

    #[test]
    fn test_tool_message_requires_tool_call_id() {
        // Construct via from_parts_unchecked then validate manually
        let msg = Message::from_parts_unchecked(
            MessageRole::Tool,
            Some("some result".to_string()),
            None,
            None,
        );
        let err = msg
            .validate()
            .expect_err("Tool message without tool_call_id must fail");
        assert!(err.to_string().contains("tool_call_id"));
    }

    #[test]
    fn test_tool_message_requires_content() {
        // Build via deserialization — missing content
        let err = serde_json::from_value::<Message>(serde_json::json!({
            "role": "tool",
            "tool_call_id": "call_abc"
        }))
        .expect_err("Tool message without content must fail validation");
        assert!(err.to_string().contains("content"));
    }

    #[test]
    fn test_assistant_tool_calls_rejects_empty_vec() {
        Message::assistant_tool_calls(vec![])
            .expect_err("Empty tool_calls vec must fail validation");
    }

    #[test]
    fn test_assistant_cannot_mix_tool_calls_and_content() {
        // Deserialization path: both content and tool_calls present
        let err = serde_json::from_value::<Message>(serde_json::json!({
            "role": "assistant",
            "content": "hello",
            "tool_calls": [{
                "id": "c1",
                "type": "function",
                "function": {"name": "f", "arguments": "{}"}
            }]
        }))
        .expect_err("Assistant message with both content and tool_calls must fail");
        assert!(err.to_string().contains("exactly one"));
    }

    #[test]
    fn test_tool_call_delta_streaming_accumulation() {
        let mut msg = Message::from_parts_unchecked(MessageRole::Assistant, None, None, None);

        // Simulate OpenAI streaming: index 0 first chunk (name), then args
        let chunk1 = serde_json::json!({"index": 0, "id": "call_aaa", "type": "function",
                                        "function": {"name": "weather", "arguments": ""}});
        let chunk2 = serde_json::json!({"index": 0, "function": {"arguments": "{\"city\":\""}});
        let chunk3 = serde_json::json!({"index": 0, "function": {"arguments": "London\"}"}});
        // Index 1 — a second parallel call
        let chunk4 = serde_json::json!({"index": 1, "id": "call_bbb", "type": "function",
                                        "function": {"name": "stock", "arguments": "{}"}});

        msg.merge_tool_call_delta(0, &chunk1);
        msg.merge_tool_call_delta(0, &chunk2);
        msg.merge_tool_call_delta(0, &chunk3);
        msg.merge_tool_call_delta(1, &chunk4);
        msg.finalize_tool_calls();

        let calls = msg
            .tool_calls()
            .expect("should have 2 tool calls after finalization");
        assert_eq!(calls.len(), 2, "expected 2 calls, got {}", calls.len());
        assert_eq!(calls[0].id(), "call_aaa");
        assert_eq!(calls[0].function().name(), "weather");
        assert_eq!(calls[0].function().arguments(), "{\"city\":\"London\"}");
        assert_eq!(calls[1].id(), "call_bbb");
        assert_eq!(calls[1].function().name(), "stock");
    }

    #[test]
    fn test_merge_chunk_message_accumulates_streamed_content_and_function_calls() {
        let mut message = Message::from_parts_unchecked(MessageRole::Assistant, None, None, None);
        let first_content = json!({ "content": "Hello" });
        let second_content = json!({ "content": " world" });
        let function_name = json!({ "function_call": { "name": "lookup_docs" } });
        let function_args_1 = json!({ "function_call": { "arguments": "{\"query\":\"ru" } });
        let function_args_2 = json!({ "function_call": { "arguments": "st\"}" } });

        merge_chunk_message(&mut message, first_content.as_object().unwrap());
        merge_chunk_message(&mut message, second_content.as_object().unwrap());

        assert_eq!(message.content(), Some("Hello world"));

        merge_chunk_message(&mut message, function_name.as_object().unwrap());
        merge_chunk_message(&mut message, function_args_1.as_object().unwrap());
        merge_chunk_message(&mut message, function_args_2.as_object().unwrap());

        let function_call = message
            .function_call()
            .expect("Expected streamed function call fragments to accumulate");
        assert_eq!(function_call.name(), "lookup_docs");
        assert_eq!(function_call.arguments(), "{\"query\":\"rust\"}");
    }
}
