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
        assert!(double_payload
            .to_string()
            .contains("either content or a function call"));

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
