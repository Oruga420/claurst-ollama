//! Ollama adapter — translates between Anthropic Messages API and Ollama's
//! OpenAI-compatible chat completions API.
//!
//! Ollama exposes `POST /v1/chat/completions` which accepts OpenAI format.
//! This adapter reuses the codex_adapter translation logic but adds:
//! - Tool/function calling support (Gemma 4 native tool use)
//! - Proper tool_calls response parsing
//! - Ollama-specific endpoint configuration

use serde_json::{json, Value};
use super::types::{ApiToolDefinition, CreateMessageRequest, CreateMessageResponse, SystemPrompt};
use claurst_core::types::UsageInfo;

/// Default Ollama API endpoint (OpenAI-compatible)
pub const OLLAMA_CHAT_ENDPOINT: &str = "http://localhost:11434/v1/chat/completions";

/// Default Ollama model
pub const OLLAMA_DEFAULT_MODEL: &str = "gemma4:e2b";

/// Convert an Anthropic CreateMessageRequest to Ollama/OpenAI chat completions format.
/// Unlike the codex adapter, this includes tool definitions for function calling.
pub fn anthropic_to_ollama_request(request: &CreateMessageRequest) -> Value {
    let mut openai_messages = vec![];

    // System prompt
    if let Some(system) = &request.system {
        let system_text = match system {
            SystemPrompt::Text(text) => text.clone(),
            SystemPrompt::Blocks(blocks) => {
                blocks
                    .iter()
                    .map(|b| b.text.clone())
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        };
        openai_messages.push(json!({
            "role": "system",
            "content": system_text,
        }));
    }

    // Convert messages — handle both text and tool_result/tool_use blocks
    for msg in &request.messages {
        let role = msg.role.to_lowercase();

        // Check if content contains tool_use or tool_result blocks
        if let Some(arr) = msg.content.as_array() {
            let mut text_parts = vec![];
            let mut tool_calls = vec![];
            let mut tool_results = vec![];

            for block in arr {
                let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match block_type {
                    "text" => {
                        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                            text_parts.push(t.to_string());
                        }
                    }
                    "tool_use" => {
                        let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                        let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                        let input = block.get("input").cloned().unwrap_or(json!({}));
                        tool_calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": serde_json::to_string(&input).unwrap_or_default(),
                            }
                        }));
                    }
                    "tool_result" => {
                        let tool_use_id = block.get("tool_use_id").and_then(|i| i.as_str()).unwrap_or("");
                        let content = block.get("content").and_then(|c| c.as_str())
                            .or_else(|| block.get("content").and_then(|c| {
                                if let Some(arr) = c.as_array() {
                                    arr.first().and_then(|b| b.get("text")).and_then(|t| t.as_str())
                                } else {
                                    None
                                }
                            }))
                            .unwrap_or("");
                        tool_results.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": content,
                        }));
                    }
                    _ => {
                        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                            text_parts.push(t.to_string());
                        }
                    }
                }
            }

            // Assistant message with tool calls
            if !tool_calls.is_empty() {
                let mut msg_obj = json!({
                    "role": "assistant",
                    "tool_calls": tool_calls,
                });
                if !text_parts.is_empty() {
                    msg_obj["content"] = json!(text_parts.join("\n"));
                }
                openai_messages.push(msg_obj);
            }
            // Tool results as separate messages
            for tr in &tool_results {
                openai_messages.push(tr.clone());
            }
            // Plain text
            if tool_calls.is_empty() && tool_results.is_empty() && !text_parts.is_empty() {
                openai_messages.push(json!({
                    "role": role,
                    "content": text_parts.join("\n"),
                }));
            }
        } else {
            // Simple string content
            openai_messages.push(json!({
                "role": role,
                "content": msg.content,
            }));
        }
    }

    // Build request
    let mut ollama_req = json!({
        "model": request.model,
        "messages": openai_messages,
        "stream": request.stream,
    });

    // Max tokens
    if request.max_tokens > 0 {
        ollama_req["max_tokens"] = json!(request.max_tokens);
    }

    // Temperature
    if let Some(temp) = request.temperature {
        ollama_req["temperature"] = json!(temp);
    }
    if let Some(top_p) = request.top_p {
        ollama_req["top_p"] = json!(top_p);
    }

    // Tools — convert Anthropic tool definitions to OpenAI function format
    if let Some(tools) = &request.tools {
        let openai_tools: Vec<Value> = tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.input_schema,
                    }
                })
            })
            .collect();

        if !openai_tools.is_empty() {
            ollama_req["tools"] = json!(openai_tools);
        }
    }

    ollama_req
}

/// Parse an Ollama/OpenAI chat completions response.
/// Handles both plain text responses AND tool_calls.
/// Returns (content_blocks_json, stop_reason, input_tokens, output_tokens)
pub fn parse_ollama_response(response: &Value) -> (Vec<Value>, String, u64, u64) {
    let choice = response
        .get("choices")
        .and_then(|c| c.get(0))
        .cloned()
        .unwrap_or(json!({}));

    let message = choice.get("message").cloned().unwrap_or(json!({}));

    let finish_reason = choice
        .get("finish_reason")
        .and_then(|f| f.as_str())
        .unwrap_or("stop");

    // Check for tool calls first
    let mut content_blocks = vec![];
    let stop_reason;

    if let Some(tool_calls) = message.get("tool_calls").and_then(|tc| tc.as_array()) {
        // Model wants to use tools
        stop_reason = "tool_use".to_string();

        // Add any text content first
        if let Some(text) = message.get("content").and_then(|c| c.as_str()) {
            if !text.is_empty() {
                content_blocks.push(json!({
                    "type": "text",
                    "text": text,
                }));
            }
        }

        // Add tool_use blocks
        for tc in tool_calls {
            let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("call_0");
            let function = tc.get("function").cloned().unwrap_or(json!({}));
            let name = function.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args_str = function.get("arguments").and_then(|a| a.as_str()).unwrap_or("{}");

            let input: Value = serde_json::from_str(args_str).unwrap_or(json!({}));

            content_blocks.push(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input,
            }));
        }
    } else {
        // Plain text response
        let text = message
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("");

        content_blocks.push(json!({
            "type": "text",
            "text": text,
        }));

        stop_reason = match finish_reason {
            "stop" => "end_turn",
            "length" => "max_tokens",
            "tool_calls" => "tool_use",
            _ => "end_turn",
        }
        .to_string();
    }

    // Usage
    let input_tokens = response
        .get("usage")
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);

    let output_tokens = response
        .get("usage")
        .and_then(|u| u.get("completion_tokens"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);

    (content_blocks, stop_reason, input_tokens, output_tokens)
}

/// Build an Anthropic CreateMessageResponse from parsed Ollama data.
/// Unlike the codex adapter, this properly handles tool_use content blocks.
pub fn build_anthropic_response(
    content_blocks: Vec<Value>,
    stop_reason: &str,
    input_tokens: u64,
    output_tokens: u64,
    model: &str,
) -> CreateMessageResponse {
    let id = format!(
        "msg_ollama_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| format!("{:x}", d.as_nanos()))
            .unwrap_or_else(|_| "0".to_string())
    );

    CreateMessageResponse {
        id,
        response_type: "message".to_string(),
        role: "assistant".to_string(),
        content: content_blocks,
        model: model.to_string(),
        stop_reason: Some(stop_reason.to_string()),
        stop_sequence: None,
        usage: UsageInfo {
            input_tokens,
            output_tokens,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        },
    }
}

/// Check if Ollama is running and the target model is available.
pub async fn check_ollama_health(base_url: &str, model: &str) -> Result<bool, String> {
    let client = reqwest::Client::new();
    let tags_url = format!("{}/api/tags", base_url.trim_end_matches("/v1/chat/completions"));

    let resp = client
        .get(&tags_url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Cannot reach Ollama at {}: {}", base_url, e))?;

    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("Invalid Ollama response: {}", e))?;

    let models = body
        .get("models")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();

    let model_base = model.split(':').next().unwrap_or(model);
    let found = models.iter().any(|m| {
        let name = m.get("name").and_then(|n| n.as_str()).unwrap_or("");
        name == model || name.starts_with(model_base)
    });

    if !found {
        let available: Vec<String> = models
            .iter()
            .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
            .collect();
        return Err(format!(
            "Model '{}' not found. Available: {:?}",
            model, available
        ));
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ApiMessage, ApiToolDefinition};

    #[test]
    fn test_anthropic_to_ollama_with_tools() {
        let request = CreateMessageRequest {
            model: "gemma4:e2b".to_string(),
            max_tokens: 1024,
            messages: vec![ApiMessage {
                role: "user".to_string(),
                content: json!("Add a meeting tomorrow at 3pm"),
            }],
            system: Some(SystemPrompt::Text("You are Luna, a Google Workspace assistant.".to_string())),
            tools: Some(vec![ApiToolDefinition {
                name: "gws_calendar_add".to_string(),
                description: "Create a Google Calendar event".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"},
                        "date": {"type": "string"},
                        "time": {"type": "string"}
                    },
                    "required": ["title", "date"]
                }),
                cache_control: None,
            }]),
            temperature: Some(0.1),
            top_p: None,
            top_k: None,
            stop_sequences: None,
            stream: false,
            thinking: None,
        };

        let ollama_req = anthropic_to_ollama_request(&request);

        assert_eq!(ollama_req["model"], "gemma4:e2b");
        assert!(ollama_req["tools"].is_array());
        let tools = ollama_req["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "gws_calendar_add");
    }

    #[test]
    fn test_parse_ollama_tool_call_response() {
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "gws_calendar_add",
                            "arguments": "{\"title\":\"Team Meeting\",\"date\":\"2026-04-09\",\"time\":\"15:00\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 150,
                "completion_tokens": 30
            }
        });

        let (blocks, stop_reason, input, output) = parse_ollama_response(&response);

        assert_eq!(stop_reason, "tool_use");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "tool_use");
        assert_eq!(blocks[0]["name"], "gws_calendar_add");
        assert_eq!(blocks[0]["input"]["title"], "Team Meeting");
        assert_eq!(input, 150);
        assert_eq!(output, 30);
    }

    #[test]
    fn test_parse_ollama_text_response() {
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "I've added the event to your calendar."
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 10
            }
        });

        let (blocks, stop_reason, _, _) = parse_ollama_response(&response);

        assert_eq!(stop_reason, "end_turn");
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "I've added the event to your calendar.");
    }
}
