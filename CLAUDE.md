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

## MemPalace + Self-Learning System

- **MemPalaceTool** (`mempalace_tool.rs`) — Local-first memory palace with wings/rooms/drawers, knowledge graph, 4-layer context, and pattern extraction
- **`/arise`** skill — Self-improving wiki + memory system (Personal/Laboral modes) that compounds knowledge across sessions
- **`/karpathy`** skill — Autonomous experimentation loop (modify → measure → keep/discard) with MemPalace integration

### Palace Storage

All memory data lives in `{project}/.claude/palace/`:
- `wings.json` — Wing index
- `wings/{name}/{room}/*.json` — Drawer files (verbatim content)
- `knowledge_graph.json` — Temporal entity triples
- `identity.txt` — L0 identity layer
- `learnings.json` — Extracted patterns

### MemPalace Tool Actions

`init`, `add_drawer`, `search`, `list_wings`, `list_rooms`, `status`, `kg_add`, `kg_query`, `kg_invalidate`, `extract_memories`, `get_layers`, `delete_drawer`

## Docs

- [docs/SETUP.md](docs/SETUP.md) — Install guide (macOS, Linux)
- [docs/OLLAMA_ADAPTER.md](docs/OLLAMA_ADAPTER.md) — Adapter internals
- [docs/GWS_TOOL.md](docs/GWS_TOOL.md) — Google Workspace tool
- [docs/ADDING_TOOLS.md](docs/ADDING_TOOLS.md) — How to add tools
- [docs/AGENT_GUIDE.md](docs/AGENT_GUIDE.md) — Full guide for AI agents
- [docs/MEMPALACE.md](docs/MEMPALACE.md) — MemPalace memory system
