# OpenAI Mock

OpenAI Mock is a tool designed to simulate the behavior of the OpenAI API for testing and development purposes without incurring costs or using actual API tokens.

## Table of Contents

- [Installation](#installation)
- [Usage](#usage)
  - [Example 1: Basic Setup](#example-1-basic-setup)
  - [Example 2: Custom Responses](#example-2-custom-responses)
  - [Example 3: Integrating with Actix-Web](#example-3-integrating-with-actix-web)
- [Running Tests](#running-tests)
- [Contributing](#contributing)
- [License](#license)
- [Contact](#contact)

## Installation

To install and set up the OpenAI Mock project, follow these steps:

1. **Clone the Repository**

   ```bash
   git clone https://github.com/socrates8300/openai-mock.git
   cd openai-mock
   ```

2. **Build the Project**

   Ensure you have [Rust](https://www.rust-lang.org/tools/install) installed. Then, build the project using Cargo:

   ```bash
   cargo build --release
   ```

3. **Run the Mock Server**

   ```bash
   cargo run --release
   ```

   By default, the server runs on `http://localhost:8000`. You can customize the port using environment variables or configuration files as needed.

## Usage

### Example 1: Basic Setup

This example demonstrates how to set up and run the OpenAI Mock server with default settings.

1. **Start the Server**

   ```bash
   cargo run --release
   ```

2. **Send a Request to the Mock Endpoint**

   You can use `curl` or any HTTP client to interact with the mock API.

   ```bash
   curl -X POST http://localhost:8000/v1/completions \
     -H "Content-Type: application/json" \
     -d '{
       "model": "gpt-3.5-turbo",
       "prompt": "Hello, world!",
       "max_tokens": 50
     }'
   ```

3. **Expected Response**

   ```json
   {
     "id": "cmpl-mock-id-<uuid>",
     "object": "text_completion",
     "created": <timestamp>,
     "model": "gpt-3.5-turbo",
     "choices": [
       {
         "text": "Hello! How can I assist you today?",
         "index": 0,
         "logprobs": null,
         "finish_reason": "stop"
       }
     ],
     "usage": {
       "prompt_tokens": 2,
       "completion_tokens": 10,
       "total_tokens": 12
     }
   }
   ```

### Example 2: Custom Responses

Customize the responses of the mock server to simulate different scenarios.

1. **Modify the Handler**

   Update the `completions_handler` in `src/handlers/completion_handler.rs` to return a custom response based on the input prompt.

   ```rust:src/handlers/completion_handler.rs
   // Inside the completions_handler function

   let response = if req.prompt.as_deref() == Some("test") {
       CompletionResponse {
           id: "cmpl-custom-id".to_string(),
           object: "text_completion".to_string(),
           created: get_current_timestamp().timestamp() as u64,
           model: req.model.clone(),
           choices: vec![
               Choice {
                   text: "This is a custom response for testing.".to_string(),
                   index: 0,
                   logprobs: None,
                   finish_reason: "stop".to_string(),
               }
           ],
           usage: Usage {
               prompt_tokens: count_tokens(&prompt),
               completion_tokens: 7,
               total_tokens: count_tokens(&prompt) + 7,
           },
       }
   } else {
       // Default mock response
       // ...
   };
   ```

2. **Restart the Server**

   ```bash
   cargo run --release
   ```

3. **Send a Custom Request**

   ```bash
   curl -X POST http://localhost:8000/v1/completions \
     -H "Content-Type: application/json" \
     -d '{
       "model": "gpt-3.5-turbo",
       "prompt": "test",
       "max_tokens": 50
     }'
   ```

4. **Expected Custom Response**

   ```json
   {
     "id": "cmpl-custom-id",
     "object": "text_completion",
     "created": <timestamp>,
     "model": "gpt-3.5-turbo",
     "choices": [
       {
         "text": "This is a custom response for testing.",
         "index": 0,
         "logprobs": null,
         "finish_reason": "stop"
       }
     ],
     "usage": {
       "prompt_tokens": 1,
       "completion_tokens": 7,
       "total_tokens": 8
     }
   }
   ```

### Example 3: Integrating with Actix-Web

Integrate the OpenAI Mock library into an existing Actix-Web application.

1. **Add Dependency**

   In your project's `Cargo.toml`, add `openai-mock` as a dependency:

   ```toml
   [dependencies]
   openai-mock = { path = "../openai-mock" } # Adjust the path as necessary
   actix-web = "4"
   serde_json = "1.0"
   ```

2. **Configure Routes**

   Update your `src/main.rs` or equivalent to include the mock routes.

   ```rust:src/main.rs
   use actix_web::{App, HttpServer};
   use openai_mock::routes::configure_completion_routes;

   #[actix_web::main]
   async fn main() -> std::io::Result<()> {
       HttpServer::new(|| {
           App::new()
               .configure(configure_completion_routes)
       })
       .bind(("127.0.0.1", 8000))?
       .run()
       .await
   }
   ```

3. **Run Your Application**

   ```bash
   cargo run
   ```

4. **Interact with the Mock API**

   Send requests to your Actix-Web application as shown in Example 1.

## Running Tests

OpenAI Mock includes a suite of tests to ensure its functionality. To run the tests:

```bash
cargo test
```

### Example Test Case

Here's an example test case located in `src/tests.rs` that verifies the completions handler.

```rust:src/tests.rs
#[cfg(test)]
mod tests {
use actix_web::{test, App};
use crate::handlers::completions_handler;
use crate::models::completion::CompletionRequest;
use serde_json::json;

#[actix_web::test]
async fn test_completions_handler() {
    // Initialize the mock service
    let app = test::init_service(
        App::new()
            .service(
                actix_web::web::resource("/v1/completions")
                    .route(actix_web::web::post().to(completions_handler)),
            )
    ).await;

    // Create a sample CompletionRequest
    let req_payload = CompletionRequest {
        model: "gpt-3.5-turbo".to_string(),
        prompt: Some(json!("Hello, world!")),
        ..Default::default()
    };

    // Create POST request
    let req = test::TestRequest::post()
        .uri("/v1/completions")
        .set_json(&req_payload)
        .to_request();

    // Send request and get the response
    let resp = test::call_service(&app, req).await;

    // Assert the response status is 200 OK
    assert!(resp.status().is_success());

    // Parse the response body
    let response_body: serde_json::Value = test::read_body_json(resp).await;

    // Assert the response contains expected fields
    assert_eq!(response_body["model"], "gpt-3.5-turbo");
    assert!(response_body["choices"].is_array());
    // Add more assertions as needed
}
}
```

### Adding New Tests

To add new tests:

1. **Create a New Test Function**

   Add a new `#[actix_web::test]` function in `src/tests.rs` or create additional test modules as needed.

2. **Write Test Logic**

   Utilize Actix-Web's testing utilities to initialize the service, send requests, and assert responses.

3. **Run Tests**

   ```bash
   cargo test
   ```

## Contributing

Contributions are welcome! Please follow these steps to contribute:

1. **Fork the Repository**

   Click the [Fork](https://github.com/socrates8300/openai-mock/fork) button at the top right corner of the repository page.

2. **Clone Your Fork**

   ```bash
   git clone https://github.com/your-username/openai-mock.git
   cd openai-mock
   ```

3. **Create a New Branch**

   ```bash
   git checkout -b feature/your-feature-name
   ```

4. **Make Your Changes**

   Implement your feature or bug fix.

5. **Commit Your Changes**

   ```bash
   git commit -m "Add feature: your feature description"
   ```

6. **Push to Your Fork**

   ```bash
   git push origin feature/your-feature-name
   ```

7. **Create a Pull Request**

   Navigate to the original repository and click the "Compare & pull request" button to submit your changes for review.

## License

This project is licensed under the [MIT License](LICENSE).

## Contact

For any questions or suggestions, please contact:

- **James T. Ray**
  - Email: [raymac@ievolution.com](mailto:raymac@ievolution.com)
  - GitHub: [socrates8300](https://github.com/socrates8300)

---

Thank you for using OpenAI Mock! We hope it aids in your development and testing endeavors.