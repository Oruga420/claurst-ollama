# Agent Implementation Guide

This guide is for **AI agents** (Claude Code, Cursor, Copilot, etc.) that need to set up and work with this codebase. Follow these instructions to get CLAURST running with Ollama on a user's machine.

## What You're Building

CLAURST is a Rust-based AI agent harness. It runs a loop:
1. User gives input (text or voice)
2. Local LLM (via Ollama) decides what tools to call
3. Tools execute (bash, file I/O, GWS CLI, etc.)
4. Results feed back to the LLM
5. LLM continues or finishes

Your job: get this running on the user's hardware with their preferred local model.

## Step 1: Assess the Environment

Before doing anything, determine:

```bash
# What OS?
uname -a

# What GPU?
# NVIDIA:
nvidia-smi 2>/dev/null || echo "No NVIDIA GPU"
# Apple Silicon:
sysctl -n machdep.cpu.brand_string 2>/dev/null || echo "Not macOS"
# AMD:
rocm-smi 2>/dev/null || echo "No AMD ROCm"

# How much VRAM / unified memory?
# NVIDIA: nvidia-smi --query-gpu=memory.total --format=csv
# macOS: sysctl -n hw.memsize (divide by 1073741824 for GB)

# Is Rust installed?
rustc --version 2>/dev/null || echo "No Rust"

# Is Ollama installed?
ollama --version 2>/dev/null || echo "No Ollama"
```

## Step 2: Install Dependencies

### Rust
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env
```

### Ollama
```bash
# macOS
brew install ollama

# Linux
curl -fsSL https://ollama.com/install.sh | sh
```

### Build dependencies (Linux only)
```bash
sudo apt-get install -y build-essential pkg-config libssl-dev
```

## Step 3: Choose the Right Model

Based on available VRAM:

| VRAM | Command | Model |
|------|---------|-------|
| 4GB | `ollama pull gemma4:e2b` | Gemma 4 E2B (2.3B active) |
| 4GB | `ollama pull qwen3:1.7b` | Qwen3 1.7B |
| 6GB | `ollama pull llama3.2:3b` | Llama 3.2 3B |
| 8GB | `ollama pull qwen3:8b` | Qwen3 8B (best quality) |
| 8GB | `ollama pull gemma4:e4b` | Gemma 4 E4B |
| 8GB (Mac) | `ollama pull qwen3:8b` | Uses unified memory |
| 16GB+ | `ollama pull qwen3:32b` | Maximum quality |

**For tool calling**, prefer: `gemma4:e2b`, `gemma4:e4b`, `qwen3:8b`, `qwen3:4b`. These have native function calling support.

## Step 4: Start Ollama and Pull Model

```bash
# Start server (skip if already running)
ollama serve &
sleep 3

# Pull model
ollama pull gemma4:e2b

# Verify
curl -s http://localhost:11434/api/tags | grep -o '"name":"[^"]*"'
```

## Step 5: Build CLAURST

```bash
cd claurst-ollama/src-rust
cargo build --release 2>&1
```

**If build fails** with Application Control or permission errors on Windows, use `cargo build` (debug mode) instead of `--release`.

**If build fails** with OpenSSL errors on Linux:
```bash
sudo apt-get install -y libssl-dev pkg-config
```

## Step 6: Configure

Create a shell alias or export env vars:

```bash
# Add to ~/.bashrc or ~/.zshrc
export OLLAMA_ENDPOINT=http://localhost:11434/v1/chat/completions
export CLAUDE_MODEL=gemma4:e2b
alias claurst='~/claurst-ollama/src-rust/target/release/claurst'
```

## Step 7: Verify Tool Calling Works

Test that the model can invoke tools via the Ollama OpenAI-compatible API:

```bash
curl -s http://localhost:11434/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemma4:e2b",
    "messages": [
      {"role": "system", "content": "You have a tool called get_weather. Call it when asked about weather."},
      {"role": "user", "content": "What is the weather in Tokyo?"}
    ],
    "tools": [{
      "type": "function",
      "function": {
        "name": "get_weather",
        "description": "Get current weather",
        "parameters": {
          "type": "object",
          "properties": {
            "city": {"type": "string"}
          },
          "required": ["city"]
        }
      }
    }]
  }'
```

Expected: response should contain `tool_calls` with `get_weather` and `{"city": "Tokyo"}`.

## Step 8: Add GWS CLI (Optional)

If the user wants Google Workspace integration:

```bash
# Install GWS CLI
npm install -g @googleworkspace/cli

# Authenticate
gws auth login   # Opens browser

# Verify
gws auth status | grep token_valid
```

The `gws` tool is already registered in CLAURST. Once `gws` is on PATH and authenticated, the LLM can use it.

## Architecture Reference

```
┌────────────────────────────────────────────────┐
│                 claurst-cli                      │
│  main.rs: CLI args → Config → bootstrap         │
└─────────────────┬──────────────────────────────┘
                  │
┌─────────────────▼──────────────────────────────┐
│               claurst-query                      │
│  Agentic loop: prompt → API → tool → repeat     │
│  Auto-compact when context > 80%                 │
│  Max turns configurable (default 10)             │
└─────────────────┬──────────────────────────────┘
                  │
┌─────────────────▼──────────────────────────────┐
│               claurst-api                        │
│  Provider router:                                │
│    Anthropic → api.anthropic.com                 │
│    Codex    → chatgpt.com                        │
│    Ollama   → localhost:11434 (ollama_adapter)   │
│                                                  │
│  ollama_adapter.rs:                              │
│    Anthropic Messages ←→ OpenAI Chat format      │
│    tool_use blocks ←→ function calls             │
└─────────────────┬──────────────────────────────┘
                  │
┌─────────────────▼──────────────────────────────┐
│               claurst-tools                      │
│  40+ tools the LLM can invoke:                   │
│    bash, file_read, file_write, file_edit        │
│    glob, grep, web_fetch, web_search             │
│    gws (Google Workspace CLI)                    │
│    agent (sub-agents), team (parallel agents)    │
│    tasks, cron, worktree, mcp, etc.              │
└────────────────────────────────────────────────┘
```

## Key Files to Know

| File | What It Does |
|------|-------------|
| `crates/api/src/ollama_adapter.rs` | Translates Anthropic ↔ Ollama format |
| `crates/api/src/lib.rs` | API client, Provider enum, request routing |
| `crates/tools/src/gws_tool.rs` | Google Workspace CLI tool |
| `crates/tools/src/lib.rs` | Tool registry (add new tools here) |
| `crates/query/src/lib.rs` | The main agentic loop |
| `crates/core/src/lib.rs` | Config, types, permissions |
| `crates/cli/src/main.rs` | Entry point |

## Common Agent Tasks

### "User wants to add a new tool"
→ See [ADDING_TOOLS.md](ADDING_TOOLS.md)

### "User wants to change the model"
→ `export CLAUDE_MODEL=<new-model>` and restart

### "User wants to add a new provider"
→ Create `my_adapter.rs` in `crates/api/src/`, add to Provider enum, wire into `create_message()`

### "Build fails"
→ Check Rust version (`rustc --version`, need 1.75+), check OpenSSL headers, check `Cargo.lock` exists

### "Model doesn't call tools"
→ Not all models support function calling. Use `gemma4:e2b`, `qwen3:8b`, or `qwen3:4b`.

### "Context too long"
→ CLAURST auto-compacts at 80% of context window. For small models with 4K context, set `max_tokens` lower.
