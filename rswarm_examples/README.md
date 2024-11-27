# RSwarm Examples

This contains an example implementation of the RSwarm framework, demonstrating how to build AI-powered applications using Rust.

## Overview

RSwarm Examples showcases practical applications of the RSwarm framework, including:
- Multi-agent interactions
- Documentation browsing capabilities
- Structured prompt execution
- Environment configuration

## Prerequisites

- Rust (latest stable version)
- An OpenAI API key
- Chrome/Chromium (for headless browser functionality)

## Installation

1. Clone the repository
2. Create a `.env` file in the project root with:

```env
OPENAI_API_KEY=<your-openai-api-key>
OPENAI_MODEL=gpt-4  # Optional, defaults to gpt-4
```

## Project Structure

- `src/main.rs` - Main application demonstrating multi-agent setup
- `src/browse_docs.rs` - Documentation browser implementation
- `prompt.txt` - Example structured prompt for agent interactions

## Features

### Documentation Browser Agent
Automatically browses and extracts information from Rust documentation using headless Chrome:
- Struct listings
- Function listings
- API documentation parsing

### Multi-Agent System
Implements three distinct agents:
1. Primary Agent - Executes main tasks
2. Agent Manager - Reviews and provides feedback (CTO role)
3. Doc Browser Agent - Handles documentation queries

## Usage

Run the example:
```bash
cargo run 
```

This will execute a multi-agent workflow that:
1. Browses Rust documentation
2. Generates implementation ideas
3. Reviews and improves the suggestions
4. Creates a final article

## Dependencies

Key dependencies include:
- `rswarm`: Core framework
- `headless_chrome`: Browser automation
- `tokio`: Async runtime
- `anyhow`: Error handling
- `dotenv`: Environment configuration
