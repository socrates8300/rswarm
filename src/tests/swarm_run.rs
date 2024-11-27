#![allow(unused)]
use crate::core::Swarm;
use crate::types::{Agent, ContextVariables, Instructions, Message};
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use openai_mock::routes::configure_completion_routes;
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;

/// Custom handler for the `/completions` endpoint.
/// It returns a predefined response when the prompt is "Hello!".
async fn completions_mock_handler(req_body: web::Json<Value>) -> impl Responder {
    // Extract the prompt from the request
    let prompt = req_body.get("prompt");

    if let Some(Value::String(prompt_str)) = prompt {
        if prompt_str == "Hello!" {
            // Return the predefined assistant response along with agent details
            let response = json!({
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "Hi there! How can I assist you today?"
                        }
                    }
                ],
                "agent": {
                    "name": "test_agent",
                    "model": "gpt-4",
                    "instructions": {
                        "text": "You are a helpful assistant."
                    },
                    "functions": [],
                    "function_call": null,
                    "parallel_tool_calls": false
                }
            });
            return HttpResponse::Ok().json(response);
        }
    }

    // Default response for any other prompts
    let default_response = json!({
        "choices": [
            {
                "message": {
                    "role": "assistant",
                    "content": "I'm not sure how to respond to that."
                }
            }
        ],
        "agent": null
    });

    HttpResponse::Ok().json(default_response)
}

/// Sets up the mock server using `actix-web` on localhost:8000
/// with a custom handler for the `/completions` endpoint.
async fn setup_mock_server() -> anyhow::Result<actix_web::dev::Server> {
    let server = HttpServer::new(|| {
        App::new()
            .configure(configure_completion_routes)
            .route("/completions", web::post().to(completions_mock_handler))
    })
    .bind(("127.0.0.1", 8000))?
    .run();

    Ok(server)
}

/// Verifies that a valid Response is returned.
#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test;

    #[actix_web::test]
    async fn test_simple_conversation() -> anyhow::Result<()> {
        // Initialize the mock service
        let app = test::init_service(
            App::new()
                .configure(configure_completion_routes)
                .route("/completions", web::post().to(completions_mock_handler)),
        )
        .await;

        // Setup Agent
        let agent = Agent {
            name: "test_agent".to_string(),
            model: "gpt-4".to_string(),
            instructions: Instructions::Text("You are a helpful assistant.".to_string()),
            functions: vec![],
            function_call: None,
            parallel_tool_calls: false,
        };

        // Build Swarm with the mock server's API URL
        let swarm = Swarm::builder()
            .with_api_key("sk-test123456789".to_string()) // Can be any string for mock
            .with_api_url("http://localhost:8000".to_string())
            .with_agent(agent.clone())
            .build()?;

        // Define a simple user message
        let messages = vec![Message {
            role: "user".to_string(),
            content: Some("Hello!".to_string()),
            name: None,
            function_call: None,
        }];

        // Create request payload
        let req = test::TestRequest::post()
            .uri("/completions")
            .set_json(&json!({
                "prompt": "Hello!",
                "model": "gpt-4"
            }))
            .to_request();

        // Send request and get response
        let resp = test::call_service(&app, req).await;

        // Assert response status
        assert!(resp.status().is_success());

        // Parse response body
        let response: serde_json::Value = test::read_body_json(resp).await;

        // Assert the response contains expected content
        assert_eq!(
            response["choices"][0]["message"]["content"],
            "Hi there! How can I assist you today?"
        );
        assert_eq!(response["choices"][0]["message"]["role"], "assistant");

        Ok(())
    }
}
