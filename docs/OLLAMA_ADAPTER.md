# Ollama Adapter — Technical Guide

## Overview

The Ollama adapter (`crates/api/src/ollama_adapter.rs`) translates between Anthropic's Messages API format and Ollama's OpenAI-compatible chat completions API. This enables CLAURST's full agentic loop to work with any model running on Ollama.

## How It Works

### Request Translation (Anthropic → Ollama)

```
Anthropic CreateMessageRequest          Ollama /v1/chat/completions
─────────────────────────               ───────────────────────────
model: "gemma4:e2b"            →        model: "gemma4:e2b"
system: "You are..."           →        messages[0]: {role: "system", ...}
messages: [{role, content}]    →        messages[1..]: [{role, content}]
tools: [{name, schema}]       →        tools: [{type: "function", function: {name, parameters}}]
max_tokens: 1024               →        max_tokens: 1024
temperature: 0.1               →        temperature: 0.1
```

### Response Translation (Ollama → Anthropic)

**Plain text response:**
```
Ollama response                         Anthropic CreateMessageResponse
───────────────                         ───────────────────────────────
choices[0].message.content     →        content: [{type: "text", text: "..."}]
choices[0].finish_reason       →        stop_reason: "end_turn"
usage.prompt_tokens            →        usage.input_tokens
usage.completion_tokens        →        usage.output_tokens
```

**Tool call response:**
```
Ollama response                         Anthropic CreateMessageResponse
───────────────                         ───────────────────────────────
choices[0].message.tool_calls  →        content: [{type: "tool_use", id, name, input}]
finish_reason: "tool_calls"    →        stop_reason: "tool_use"
```

### Tool Result Round-Trip

When the model calls a tool, CLAURST:
1. Executes the tool
2. Wraps the result as an Anthropic `tool_result` block
3. The adapter converts it to an OpenAI `tool` role message
4. Sends it back to Ollama
5. The model continues with the result

```
Model: "I'll check your calendar"
Model: tool_use → gws_calendar_list
                    ↓
CLAURST executes: gws calendar events list ...
                    ↓
Tool result: [{type: "tool_result", content: "3 events found..."}]
                    ↓
Adapter converts to: {role: "tool", tool_call_id: "...", content: "3 events found..."}
                    ↓
Ollama processes result
                    ↓
Model: "You have 3 events tomorrow: ..."
```

## Key Functions

### `anthropic_to_ollama_request(request) → Value`
Converts the full Anthropic request including:
- System prompt (text or blocks)
- Message history with tool_use and tool_result blocks
- Tool definitions → OpenAI function format
- Temperature, max_tokens, top_p

### `parse_ollama_response(response) → (Vec<Value>, String, u64, u64)`
Parses Ollama's response and returns:
- Content blocks (text and/or tool_use)
- Stop reason (`end_turn` or `tool_use`)
- Token counts

### `build_anthropic_response(...) → CreateMessageResponse`
Wraps the parsed data into an Anthropic-compatible response struct.

### `check_ollama_health(base_url, model) → Result<bool>`
Verifies Ollama is running and the requested model is available.

## Provider Configuration

In `crates/api/src/lib.rs`, the `Provider` enum has three variants:

```rust
pub enum Provider {
    Anthropic,  // Cloud: api.anthropic.com
    Codex,      // Cloud: OpenAI Codex
    Ollama,     // Local: localhost:11434
}
```

The client routes to the right adapter automatically:
```rust
pub async fn create_message(&self, request: CreateMessageRequest) -> Result<...> {
    if self.config.provider == Provider::Codex {
        return self.create_message_codex(&request).await;
    }
    if self.config.provider == Provider::Ollama {
        return self.create_message_ollama(&request).await;
    }
    // Default: Anthropic
    ...
}
```

## Supported Models

Any model on Ollama that supports tool/function calling:

| Model | Tool Calling | Quality | Speed |
|-------|-------------|---------|-------|
| `gemma4:e2b` | Native | Good | Fast |
| `gemma4:e4b` | Native | Better | Medium |
| `qwen3:8b` | Native | Best (small) | Medium |
| `qwen3:4b` | Native | Good | Fast |
| `llama3.2:3b` | Native | Good | Fast |
| `qwen3:1.7b` | Native | Basic | Very fast |

Models without native tool calling will still work for text generation but won't be able to invoke tools.

## Limitations

- **No streaming yet** — Ollama provider uses synchronous requests. The agentic loop still works, but you won't see token-by-token output.
- **No thinking blocks** — Ollama doesn't support Anthropic's extended thinking format. The thinking config is silently ignored.
- **No prompt caching** — Ollama doesn't support Anthropic's `cache_control` ephemeral blocks.

## Extending

To add a new provider (e.g., a custom inference server):

1. Create `my_adapter.rs` in `crates/api/src/`
2. Implement `anthropic_to_my_request()` and `parse_my_response()`
3. Add `MyProvider` to the `Provider` enum
4. Add `create_message_my_provider()` to `AnthropicClient`
5. Wire it into `create_message()` with an `if` check
