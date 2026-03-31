# rswarm_examples

`rswarm_examples` is the workspace example crate for `rswarm`. It exercises a small multi-agent workflow, uses the XML step executor in [`prompt.txt`](prompt.txt), and demonstrates a tool-backed docs browser built on `headless_chrome`.

## What It Runs

The example wires together three agents:

- `Agent`: produces the main draft
- `AgentManager`: reviews and improves the draft
- `DocBrowserAgent`: uses the `browse_docs` tool to inspect `docs.rs`

The run loop starts with a seeded user message because `Swarm::run(...)` requires a non-empty message history.

## Prerequisites

- Rust stable
- An OpenAI-compatible API key in `OPENAI_API_KEY`
- Chrome or Chromium available locally for `headless_chrome`
- Network access to `docs.rs` and the configured chat-completions endpoint

## Configuration

From the workspace root, create a `.env` file:

```env
OPENAI_API_KEY=sk-...
OPENAI_MODEL=gpt-4o
```

`OPENAI_MODEL` is optional and defaults to `gpt-4o`.

## Running The Example

From the workspace root:

```bash
cargo run -p rswarm_examples
```

From the crate directory:

```bash
cargo run
```

The example will:

1. Load the XML prompt from `prompt.txt`
2. Ask the docs browser tool to inspect crate documentation
3. Generate an article draft
4. Review that draft with the manager agent
5. Print the final response and wait for Enter before exiting

## Important Files

- [`src/main.rs`](src/main.rs): example entry point and agent wiring
- [`src/browse_docs.rs`](src/browse_docs.rs): `docs.rs` browser tool
- [`prompt.txt`](prompt.txt): XML-driven workflow definition
- [`src/lib.rs`](src/lib.rs): smoke tests guarding the example contract

## Verifying The Example

```bash
cargo test -p rswarm_examples
```

Those tests cover the two example-specific regressions that previously caused confusion:

- `Swarm::run(...)` must reject an empty message history
- the seeded initial user message must remain valid
