#[cfg(test)]
mod tests {
    use crate::stream::Streamer;
    use crate::types::{Agent, ContextVariables, Instructions, Message};
    use crate::SwarmError;
    use futures_util::{pin_mut, StreamExt};
    use reqwest::Client;
    use std::time::Duration;
    use tokio;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Helper function to create a simple test agent.
    fn test_agent() -> Agent {
        Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("You are a helpful assistant.".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        }
    }

    #[tokio::test]
    async fn test_stream_chat_returns_messages() {
        // Start a WireMock server.
        let mock_server = MockServer::start().await;

        // Create a response body that simulates streaming chunks.
        // The first chunk returns a partial response with a choice message,
        // followed by [DONE] to indicate the end of the stream.
        let body = "data: {\"id\":\"dummy\",\"object\":\"chat.completion\",\"created\":0,\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"Hello from stream!\",\"name\":null,\"function_call\":null},\"finish_reason\":null}]}\n\
                    data: [DONE]\n";

        let response = ResponseTemplate::new(200).set_body_raw(body, "text/event-stream");

        // Set up the stub for POST /completions.
        Mock::given(method("POST"))
            .and(path("/completions"))
            .respond_with(response)
            .mount(&mock_server)
            .await;

        // Set the environment variable to point to our WireMock server.
        std::env::set_var(
            "OPENAI_API_URL",
            format!("{}/completions", &mock_server.uri()),
        );

        // Create an HTTP client.
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build client");
        let api_key = "sk-test123456789".to_string();

        let streamer = Streamer::new(client, api_key);
        let agent = test_agent();
        let history: Vec<Message> = vec![Message {
            role: "user".to_string(),
            content: Some("Hello!".to_string()),
            name: None,
            function_call: None,
        }];
        let context_variables = ContextVariables::new();

        let stream = streamer.stream_chat(&agent, &history, &context_variables, None, true);
        pin_mut!(stream);

        // Await one message from the stream.
        if let Some(result) = stream.next().await {
            match result {
                Ok(message) => {
                    // Verify that the message role and content are as expected.
                    assert_eq!(message.role, "assistant");
                    assert_eq!(message.content.unwrap(), "Hello from stream!");
                }
                Err(e) => {
                    panic!("Stream returned an error: {:?}", e);
                }
            }
        } else {
            panic!("No messages returned from the stream");
        }
    }
}
