# CLAURST + Ollama — Run Claude Code's Agent Loop with Local Models

A Rust-based AI agent harness (clean-room reimplementation of Claude Code's architecture) extended with an **Ollama adapter** for running local models like **Gemma 4**, **Qwen3**, and **Llama 3.2** with full **tool calling** support.

Run the same agentic loop that powers Claude Code — but with your own local LLM on a machine with as little as **4-8GB GPU**.

## What This Is

CLAURST is a clean-room Rust port of Claude Code's core architecture:
- **Agentic query loop** — model calls tools, gets results, calls more tools, until done
- **40+ built-in tools** — bash, file I/O, grep, glob, web fetch, MCP, sub-agents, teams
- **GWS tool** — Google Workspace CLI integration (Calendar, Gmail, Drive, Docs, Sheets, Tasks)
- **Permission system** — interactive approval or auto-approve per tool
- **Context compaction** — auto-summarize when context fills up
- **Multi-provider** — Anthropic API, OpenAI Codex, **Ollama (local)**

## The Ollama Adapter

The `ollama_adapter.rs` translates between Anthropic's Messages API format and Ollama's OpenAI-compatible API, including:

- **Full tool/function calling** — models like Gemma 4 and Qwen3 can invoke tools natively
- **Tool result round-trips** — tool_use → execute → tool_result → model continues
- **Automatic format translation** — no changes needed to the rest of the harness
- **Health checks** — verify Ollama is running and the model is loaded

## Quick Start

```bash
# 1. Install Ollama
curl -fsSL https://ollama.com/install.sh | sh

# 2. Pull a model (pick one that fits your GPU)
ollama pull gemma4:e2b      # 2.3B active params, ~4-6GB VRAM
# OR
ollama pull qwen3:4b         # 4B params, ~3GB VRAM
# OR
ollama pull llama3.2:3b      # 3B params, ~2GB VRAM

# 3. Clone and build
git clone https://github.com/Oruga420/claurst-ollama.git
cd claurst-ollama/src-rust
cargo build --release

# 4. Run with Ollama
export OLLAMA_ENDPOINT=http://localhost:11434/v1/chat/completions
export CLAUDE_MODEL=gemma4:e2b
./target/release/claurst
```

## Model Recommendations by GPU

| GPU VRAM | Model | `ollama pull` | Best For |
|----------|-------|--------------|----------|
| **4GB** | Gemma 4 E2B | `gemma4:e2b` | Tool calling, intent classification |
| **4GB** | Qwen3 1.7B | `qwen3:1.7b` | Fast classification, multilingual |
| **6GB** | Llama 3.2 3B | `llama3.2:3b` | General reasoning + tools |
| **8GB** | Qwen3 8B | `qwen3:8b` | Best small model for agentic tasks |
| **8GB** | Gemma 4 E4B | `gemma4:e4b` | Google's best at this size |

## Architecture

```
User Input
    │
    ▼
┌──────────────────────────────────────────┐
│            CLAURST Query Loop             │
│  (claurst-query/lib.rs)                  │
│                                          │
│  1. Build messages + tool definitions    │
│  2. Send to provider (Ollama)            │
│  3. Parse response (text or tool_use)    │
│  4. If tool_use → execute tool           │
│  5. Feed result back → goto 2            │
│  6. If end_turn → done                   │
│  7. Auto-compact if context fills up     │
└──────────┬───────────────────────────────┘
           │
    ┌──────▼──────┐
    │  Provider    │
    │  Router      │
    ├──────────────┤
    │ Anthropic    │ ← Claude API (cloud)
    │ Codex        │ ← OpenAI (cloud)
    │ **Ollama**   │ ← Local models (NEW)
    └──────┬──────┘
           │
    ┌──────▼──────┐
    │  Ollama      │
    │  Adapter     │
    │              │
    │ Anthropic    │
    │ Messages API │──→ OpenAI Chat
    │ format       │    Completions
    │              │    format
    │ + tool_use   │──→ + function
    │   blocks     │    calling
    └──────┬──────┘
           │
    ┌──────▼──────┐
    │  Ollama      │
    │  Server      │
    │  :11434      │
    │              │
    │  gemma4:e2b  │
    │  qwen3:8b    │
    │  etc.        │
    └─────────────┘
```

## Documentation

| Guide | For |
|-------|-----|
| [docs/SETUP.md](docs/SETUP.md) | Installation on macOS, Linux, Windows |
| [docs/OLLAMA_ADAPTER.md](docs/OLLAMA_ADAPTER.md) | How the Ollama adapter works |
| [docs/GWS_TOOL.md](docs/GWS_TOOL.md) | Google Workspace CLI tool guide |
| [docs/ADDING_TOOLS.md](docs/ADDING_TOOLS.md) | How to create custom tools |
| [docs/AGENT_GUIDE.md](docs/AGENT_GUIDE.md) | Guide for AI agents implementing this |

## Project Structure

```
src-rust/
├── Cargo.toml              # Workspace manifest
└── crates/
    ├── api/                 # HTTP client + provider adapters
    │   └── src/
    │       ├── lib.rs       # AnthropicClient + Provider enum
    │       ├── ollama_adapter.rs  # ★ NEW: Ollama/Gemma4 adapter
    │       └── codex_adapter.rs   # OpenAI Codex adapter
    ├── core/                # Config, types, permissions
    ├── tools/               # 40+ tool implementations
    │   └── src/
    │       ├── gws_tool.rs  # ★ NEW: Google Workspace CLI
    │       ├── bash.rs
    │       ├── file_read.rs
    │       └── ...
    ├── query/               # Agentic loop + compaction
    ├── tui/                 # Terminal UI (ratatui)
    ├── cli/                 # Entry point
    ├── commands/            # Slash commands
    ├── mcp/                 # MCP server integration
    ├── bridge/              # Remote sessions
    ├── buddy/               # Companion system
    └── plugins/             # Plugin system
```

## License

GPL-3.0 — See [LICENSE.md](LICENSE.md)
