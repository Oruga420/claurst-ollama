# CLAUDE.md — Agent Instructions for claurst-ollama

## What This Repo Is

CLAURST is a Rust clean-room reimplementation of Claude Code's agent architecture, extended with:
- **Ollama adapter** (`ollama_adapter.rs`) — run local LLMs instead of Claude API
- **GWS tool** (`gws_tool.rs`) — Google Workspace CLI as a native tool

## Key Architecture

- `crates/api/` — HTTP client with 3 providers: Anthropic, Codex, **Ollama**
- `crates/tools/` — 40+ tools including **gws** for Google Workspace
- `crates/query/` — The agentic loop (prompt → tool call → result → repeat)
- `crates/core/` — Config, types, permissions, context management

## Rules

1. **Do not modify Anthropic API behavior** — the Ollama adapter sits alongside, not replaces
2. **Tool implementations use `ToolResult::success()` / `ToolResult::error()`** — not raw struct construction
3. **New tools go in `crates/tools/src/`** and must be registered in `lib.rs`
4. **Test with `cargo check --package <crate>`** before full builds
5. **The Ollama adapter does NOT support streaming yet** — non-streaming only

## Build

```bash
cd src-rust
cargo check                    # Quick type check
cargo build --release          # Full build
cargo check --package claurst-api    # Check just the API crate
cargo check --package claurst-tools  # Check just the tools crate
```

## Environment

```bash
OLLAMA_ENDPOINT=http://localhost:11434/v1/chat/completions
CLAUDE_MODEL=gemma4:e2b
```

## Docs

- [docs/SETUP.md](docs/SETUP.md) — Install guide (macOS, Linux)
- [docs/OLLAMA_ADAPTER.md](docs/OLLAMA_ADAPTER.md) — Adapter internals
- [docs/GWS_TOOL.md](docs/GWS_TOOL.md) — Google Workspace tool
- [docs/ADDING_TOOLS.md](docs/ADDING_TOOLS.md) — How to add tools
- [docs/AGENT_GUIDE.md](docs/AGENT_GUIDE.md) — Full guide for AI agents
