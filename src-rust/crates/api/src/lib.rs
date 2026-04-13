// claurst-api: Anthropic API client with streaming SSE support for Claurst
// Rust port.
//
// Handles:
// - POST /v1/messages with streaming
// - SSE event parsing (message_start, content_block_start, content_block_delta,
//   content_block_stop, message_delta, message_stop, error)
// - Delta types: text_delta, input_json_delta, thinking_delta, signature_delta
// - Rate-limit (429) and overloaded (529) retry with exponential back-off
// - Authentication via API key from env or config

use claurst_core::constants::{ANTHROPIC_API_VERSION, ANTHROPIC_BETA_HEADER};
use claurst_core::error::ClaudeError;
use claurst_core::types::{ContentBlock, Message, MessageContent, Role, ToolDefinition, UsageInfo};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Modules
// ---------------------------------------------------------------------------
pub mod cch;
pub mod codex_adapter;
pub mod ollama_adapter;

// ---------------------------------------------------------------------------
// Public re-exports
// ---------------------------------------------------------------------------
pub use client::AnthropicClient;
pub use streaming::{StreamEvent, StreamHandler};
pub use types::*;

// ---------------------------------------------------------------------------
// request / response types
// ---------------------------------------------------------------------------
pub mod types {
    use super::*;

    /// The request body sent to `POST /v1/messages`.
    #[derive(Debug, Clone, Serialize)]
    pub struct CreateMessageRequest {
        pub model: String,
        pub max_tokens: u32,
        pub messages: Vec<ApiMessage>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub system: Option<SystemPrompt>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub tools: Option<Vec<ApiToolDefinition>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub temperature: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub top_p: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub top_k: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub stop_sequences: Option<Vec<String>>,
        pub stream: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub thinking: Option<ThinkingConfig>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ThinkingConfig {
        #[serde(rename = "type")]
        pub thinking_type: String,
        pub budget_tokens: u32,
    }

    impl ThinkingConfig {
        pub fn enabled(budget: u32) -> Self {
            Self {
                thinking_type: "enabled".to_string(),
                budget_tokens: budget,
            }
        }
    }

    /// System prompt - either a single string or structured blocks with cache.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum SystemPrompt {
        Text(String),
        Blocks(Vec<SystemBlock>),
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SystemBlock {
        #[serde(rename = "type")]
        pub block_type: String,
        pub text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub cache_control: Option<CacheControl>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CacheControl {
        #[serde(rename = "type")]
        pub control_type: String,
    }

    impl CacheControl {
        pub fn ephemeral() -> Self {
            Self {
                control_type: "ephemeral".to_string(),
            }
        }
    }

    /// Simplified message type for the API wire format.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ApiMessage {
        pub role: String,
        pub content: Value,
    }

    impl From<&Message> for ApiMessage {
        fn from(msg: &Message) -> Self {
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            let content = match &msg.content {
                MessageContent::Text(t) => Value::String(t.clone()),
                MessageContent::Blocks(blocks) => {
                    serde_json::to_value(blocks).unwrap_or(Value::Null)
                }
            };
            Self {
                role: role.to_string(),
                content,
            }
        }
    }

    /// Tool definition in the API wire format.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ApiToolDefinition {
        pub name: String,
        pub description: String,
        pub input_schema: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub cache_control: Option<CacheControl>,
    }

    impl From<&ToolDefinition> for ApiToolDefinition {
        fn from(td: &ToolDefinition) -> Self {
            Self {
                name: td.name.clone(),
                description: td.description.clone(),
                input_schema: td.input_schema.clone(),
                cache_control: None,
            }
        }
    }

    /// Non-streaming response from `POST /v1/messages`.
    #[derive(Debug, Clone, Deserialize)]
    pub struct CreateMessageResponse {
        pub id: String,
        #[serde(rename = "type")]
        pub response_type: String,
        pub role: String,
        pub content: Vec<Value>,
        pub model: String,
        pub stop_reason: Option<String>,
        pub stop_sequence: Option<String>,
        pub usage: UsageInfo,
    }

    /// Error body returned by the API.
    #[derive(Debug, Clone, Deserialize)]
    pub struct ApiErrorResponse {
        #[serde(rename = "type")]
        pub error_type: String,
        pub error: ApiErrorDetail,
    }

    #[derive(Debug, Clone, Deserialize)]
    pub struct ApiErrorDetail {
        #[serde(rename = "type")]
        pub error_type: String,
        pub message: String,
    }
}

// ---------------------------------------------------------------------------
// SSE streaming types
// ---------------------------------------------------------------------------
pub mod streaming {
    use super::*;

    /// Events emitted by the streaming SSE parser.
    #[derive(Debug, Clone)]
    pub enum StreamEvent {
        /// The overall message has started; carries the message id and model.
        MessageStart {
            id: String,
            model: String,
            usage: UsageInfo,
        },
        /// A new content block has begun.
        ContentBlockStart {
            index: usize,
            content_block: ContentBlock,
        },
        /// Incremental delta for an existing content block.
        ContentBlockDelta {
            index: usize,
            delta: ContentDelta,
        },
        /// A content block is finished.
        ContentBlockStop {
            index: usize,
        },
        /// Final message-level delta (stop_reason, usage).
        MessageDelta {
            stop_reason: Option<String>,
            usage: Option<UsageInfo>,
        },
        /// The message is complete.
        MessageStop,
        /// An error occurred during streaming.
        Error {
            error_type: String,
            message: String,
        },
        /// A ping/keep-alive event.
        Ping,
    }

    /// The delta payload inside a `content_block_delta` event.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum ContentDelta {
        TextDelta { text: String },
        InputJsonDelta { partial_json: String },
        ThinkingDelta { thinking: String },
        SignatureDelta { signature: String },
    }

    /// Trait for anything that wants to consume streaming events in real time.
    pub trait StreamHandler: Send + Sync {
        fn on_event(&self, event: &StreamEvent);
    }

    /// A no-op handler useful for non-interactive / batch mode.
    pub struct NullStreamHandler;
    impl StreamHandler for NullStreamHandler {
        fn on_event(&self, _event: &StreamEvent) {}
    }
}

// ---------------------------------------------------------------------------
// SSE line parser
// ---------------------------------------------------------------------------
mod sse_parser {
    /// Parsed SSE frame.
    #[derive(Debug)]
    pub struct SseFrame {
        pub event: Option<String>,
        pub data: String,
    }

    /// Incrementally accumulates raw bytes/lines and yields complete frames.
    pub struct SseLineParser {
        event_type: Option<String>,
        data_buf: String,
    }

    impl SseLineParser {
        pub fn new() -> Self {
            Self {
                event_type: None,
                data_buf: String::new(),
            }
        }

        /// Feed one line (without the trailing newline).  Returns `Some(frame)`
        /// when a blank line signals the end of an event.
        pub fn feed_line(&mut self, line: &str) -> Option<SseFrame> {
            if line.is_empty() {
                // Blank line = end of event
                if self.data_buf.is_empty() && self.event_type.is_none() {
                    return None; // spurious blank line
                }
                let frame = SseFrame {
                    event: self.event_type.take(),
                    data: std::mem::take(&mut self.data_buf),
                };
                return Some(frame);
            }

            if let Some(rest) = line.strip_prefix("event:") {
                self.event_type = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("data:") {
                if !self.data_buf.is_empty() {
                    self.data_buf.push('\n');
                }
                self.data_buf.push_str(rest.trim());
            } else if line.starts_with(':') {
                // SSE comment / keep-alive – ignore
            }

            None
        }
    }
}

// ---------------------------------------------------------------------------
// Models endpoint types (public)
// ---------------------------------------------------------------------------

/// A model entry returned by `GET /v1/models`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AvailableModel {
    pub id: String,
    pub display_name: Option<String>,
    /// Unix timestamp of when the model was created (seconds).
    pub created_at: Option<i64>,
}

// ---------------------------------------------------------------------------
// Anthropic client
// ---------------------------------------------------------------------------
pub mod client {
    use super::*;
    use crate::streaming::ContentDelta;

    /// Provider selection for API calls.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Provider {
        /// Anthropic (Claude Opus/Sonnet/Haiku) — native Messages API
        Anthropic,
        /// OpenAI Codex (ChatGPT backend) via OAuth
        Codex,
        /// Local Ollama (OpenAI-compatible chat completions)
        Ollama,
        /// OpenAI public API (GPT-*, o1/o3)
        OpenAI,
        /// Google Gemini via OpenAI-compatible endpoint
        Gemini,
        /// Z.ai / Zhipu GLM (open.bigmodel.cn, OpenAI-compatible)
        Zai,
        /// Moonshot Kimi (OpenAI-compatible)
        Moonshot,
        /// Alibaba Qwen via DashScope (OpenAI-compatible)
        Alibaba,
        /// DeepSeek (OpenAI-compatible)
        DeepSeek,
    }

    impl Default for Provider {
        fn default() -> Self {
            Provider::Anthropic
        }
    }

    impl Provider {
        /// Canonical short name, used for `--provider` flag and settings.
        pub fn name(&self) -> &'static str {
            match self {
                Provider::Anthropic => "anthropic",
                Provider::Codex => "codex",
                Provider::Ollama => "ollama",
                Provider::OpenAI => "openai",
                Provider::Gemini => "gemini",
                Provider::Zai => "zai",
                Provider::Moonshot => "moonshot",
                Provider::Alibaba => "alibaba",
                Provider::DeepSeek => "deepseek",
            }
        }

        /// Parse provider from CLI flag / settings string. Accepts common aliases.
        pub fn parse_name(s: &str) -> Option<Provider> {
            match s.trim().to_lowercase().as_str() {
                "anthropic" | "claude" => Some(Provider::Anthropic),
                "codex" => Some(Provider::Codex),
                "ollama" | "local" => Some(Provider::Ollama),
                "openai" | "chatgpt" | "gpt" => Some(Provider::OpenAI),
                "gemini" | "google" => Some(Provider::Gemini),
                "zai" | "glm" | "zhipu" | "bigmodel" => Some(Provider::Zai),
                "moonshot" | "kimi" => Some(Provider::Moonshot),
                "alibaba" | "qwen" | "dashscope" => Some(Provider::Alibaba),
                "deepseek" => Some(Provider::DeepSeek),
                _ => None,
            }
        }

        /// Auto-detect provider from a model string by prefix.
        pub fn from_model(model: &str) -> Provider {
            let m = model.trim().to_lowercase();
            if m.starts_with("claude") || m.starts_with("anthropic") {
                Provider::Anthropic
            } else if m.starts_with("gpt")
                || m.starts_with("o1")
                || m.starts_with("o3")
                || m.starts_with("o4")
                || m.starts_with("chatgpt")
            {
                Provider::OpenAI
            } else if m.starts_with("gemini") {
                Provider::Gemini
            } else if m.starts_with("glm") {
                Provider::Zai
            } else if m.starts_with("kimi") || m.starts_with("moonshot") {
                Provider::Moonshot
            } else if m.starts_with("qwen") {
                Provider::Alibaba
            } else if m.starts_with("deepseek") {
                Provider::DeepSeek
            } else {
                Provider::Anthropic
            }
        }

        /// Chat-completions URL for OpenAI-compatible providers. None for
        /// Anthropic (uses native Messages API) and Ollama (configured via env).
        pub fn chat_completions_url(&self) -> Option<&'static str> {
            match self {
                Provider::OpenAI => Some("https://api.openai.com/v1/chat/completions"),
                Provider::Gemini => Some(
                    "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
                ),
                Provider::Zai => {
                    Some("https://open.bigmodel.cn/api/paas/v4/chat/completions")
                }
                Provider::Moonshot => Some("https://api.moonshot.ai/v1/chat/completions"),
                Provider::Alibaba => Some(
                    "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions",
                ),
                Provider::DeepSeek => Some("https://api.deepseek.com/v1/chat/completions"),
                _ => None,
            }
        }

        /// Preferred env var for this provider's API key.
        pub fn api_key_env(&self) -> &'static str {
            match self {
                Provider::Anthropic => "ANTHROPIC_API_KEY",
                Provider::Codex => "CODEX_API_KEY",
                Provider::Ollama => "OLLAMA_API_KEY",
                Provider::OpenAI => "OPENAI_API_KEY",
                Provider::Gemini => "GEMINI_API_KEY",
                Provider::Zai => "ZAI_API_KEY",
                Provider::Moonshot => "MOONSHOT_API_KEY",
                Provider::Alibaba => "DASHSCOPE_API_KEY",
                Provider::DeepSeek => "DEEPSEEK_API_KEY",
            }
        }

        /// Default model to use when the user doesn't specify one.
        pub fn default_model(&self) -> &'static str {
            match self {
                Provider::Anthropic => "claude-opus-4-6",
                Provider::Codex => "gpt-5.2-codex",
                Provider::Ollama => "qwen3:1.7b",
                Provider::OpenAI => "gpt-5.2",
                Provider::Gemini => "gemini-2.5-pro",
                Provider::Zai => "glm-5.1",
                Provider::Moonshot => "kimi-k2.5",
                Provider::Alibaba => "qwen3.5-max",
                Provider::DeepSeek => "deepseek-v3.2",
            }
        }

        /// Whether this provider uses an OpenAI-compatible chat completions endpoint
        /// that we route through the ollama_adapter translation.
        pub fn is_openai_compat(&self) -> bool {
            self.chat_completions_url().is_some()
        }
    }

    /// Configuration for the HTTP client.
    #[derive(Debug, Clone)]
    pub struct ClientConfig {
        pub api_key: String,
        pub api_base: String,
        pub api_version: String,
        pub beta_features: String,
        pub max_retries: u32,
        pub initial_retry_delay: Duration,
        pub max_retry_delay: Duration,
        pub request_timeout: Duration,
        /// When true, send `Authorization: Bearer <api_key>` instead of `x-api-key`.
        /// Used for Claude.ai subscription (OAuth user:inference scope) tokens.
        pub use_bearer_auth: bool,
        /// Which provider to use for API calls.
        pub provider: Provider,
    }

    impl Default for ClientConfig {
        fn default() -> Self {
            Self {
                api_key: String::new(),
                api_base: claurst_core::constants::ANTHROPIC_API_BASE.to_string(),
                api_version: ANTHROPIC_API_VERSION.to_string(),
                beta_features: ANTHROPIC_BETA_HEADER.to_string(),
                max_retries: 5,
                initial_retry_delay: Duration::from_secs(1),
                max_retry_delay: Duration::from_secs(60),
                request_timeout: Duration::from_secs(600),
                use_bearer_auth: false,
                provider: Provider::Anthropic,
            }
        }
    }

    /// The main Anthropic API client.
    pub struct AnthropicClient {
        http: reqwest::Client,
        config: ClientConfig,
    }

    impl AnthropicClient {
        /// Build a new client. Requires a non-empty API key for all cloud
        /// providers; Ollama is allowed without a key since it's local.
        pub fn new(config: ClientConfig) -> anyhow::Result<Self> {
            if config.api_key.is_empty() && config.provider != Provider::Ollama {
                return Err(anyhow::anyhow!(
                    "{} API key is required. Set {} or pass --api-key.",
                    config.provider.name(),
                    config.provider.api_key_env(),
                ));
            }

            let http = reqwest::Client::builder()
                .timeout(config.request_timeout)
                .build()?;

            Ok(Self { http, config })
        }

        /// Convenience constructor that resolves the key from config/env.
        pub fn from_config(cfg: &claurst_core::config::Config) -> anyhow::Result<Self> {
            let api_key = cfg
                .resolve_api_key()
                .ok_or_else(|| anyhow::anyhow!("No API key found"))?;
            let api_base = cfg.resolve_api_base();

            Self::new(ClientConfig {
                api_key,
                api_base,
                ..Default::default()
            })
        }

        // ---- Non-streaming create message --------------------------------

        /// Send a non-streaming `POST /v1/messages` and return the full response.
        pub async fn create_message(
            &self,
            mut request: CreateMessageRequest,
        ) -> Result<CreateMessageResponse, ClaudeError> {
            // Route to alternative providers
            if self.config.provider == Provider::Codex {
                return self.create_message_codex(&request).await;
            }
            if self.config.provider == Provider::Ollama {
                return self.create_message_ollama(&request).await;
            }
            if let Some(url) = self.config.provider.chat_completions_url() {
                return self.create_message_openai_compat(&request, url).await;
            }

            request.stream = false;
            let body = serde_json::to_value(&request).map_err(ClaudeError::Json)?;

            let resp = self.send_with_retry(&body).await?;
            let status = resp.status();
            let text = resp.text().await.map_err(ClaudeError::Http)?;

            if !status.is_success() {
                return Err(self.parse_api_error(status.as_u16(), &text));
            }

            serde_json::from_str(&text).map_err(ClaudeError::Json)
        }

        /// Send a request to OpenAI Codex API instead of Anthropic.
        async fn create_message_codex(
            &self,
            request: &CreateMessageRequest,
        ) -> Result<CreateMessageResponse, ClaudeError> {
            // Convert Anthropic format to OpenAI format
            let openai_req = codex_adapter::anthropic_to_openai_request(request);

            // Send to Codex endpoint
            let client = reqwest::Client::new();
            let resp = client
                .post(codex_adapter::CODEX_RESPONSES_ENDPOINT)
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .header("Content-Type", "application/json")
                .json(&openai_req)
                .timeout(self.config.request_timeout)
                .send()
                .await
                .map_err(|e| ClaudeError::Other(format!("Codex request failed: {}", e)))?;

            let status = resp.status();
            let text = resp.text().await.map_err(ClaudeError::Http)?;

            if !status.is_success() {
                return Err(self.parse_api_error(status.as_u16(), &text));
            }

            // Parse OpenAI response and convert to Anthropic format
            let openai_resp: Value = serde_json::from_str(&text).map_err(ClaudeError::Json)?;
            let (content, stop_reason, input_tokens, output_tokens) =
                codex_adapter::parse_openai_response(&openai_resp);

            let response = codex_adapter::build_anthropic_response(
                &content,
                &stop_reason,
                input_tokens,
                output_tokens,
                &request.model,
            );

            Ok(response)
        }

        /// Send a request to local Ollama API with tool calling support.
        async fn create_message_ollama(
            &self,
            request: &CreateMessageRequest,
        ) -> Result<CreateMessageResponse, ClaudeError> {
            let ollama_req = ollama_adapter::anthropic_to_ollama_request(request);

            let endpoint = std::env::var("OLLAMA_ENDPOINT")
                .unwrap_or_else(|_| ollama_adapter::OLLAMA_CHAT_ENDPOINT.to_string());

            let client = reqwest::Client::new();
            let resp = client
                .post(&endpoint)
                .header("Content-Type", "application/json")
                .json(&ollama_req)
                .timeout(self.config.request_timeout)
                .send()
                .await
                .map_err(|e| ClaudeError::Other(format!("Ollama request failed: {}", e)))?;

            let status = resp.status();
            let text = resp.text().await.map_err(ClaudeError::Http)?;

            if !status.is_success() {
                return Err(self.parse_api_error(status.as_u16(), &text));
            }

            let ollama_resp: Value = serde_json::from_str(&text).map_err(ClaudeError::Json)?;
            let (content_blocks, stop_reason, input_tokens, output_tokens) =
                ollama_adapter::parse_ollama_response(&ollama_resp);

            Ok(ollama_adapter::build_anthropic_response(
                content_blocks,
                &stop_reason,
                input_tokens,
                output_tokens,
                &request.model,
            ))
        }

        /// Send a request to an OpenAI-compatible provider (OpenAI, Gemini,
        /// Z.ai/GLM, Moonshot/Kimi, Alibaba/Qwen, DeepSeek). Reuses
        /// `ollama_adapter` for the format translation since all of these
        /// providers accept OpenAI chat completions with tool calling.
        async fn create_message_openai_compat(
            &self,
            request: &CreateMessageRequest,
            endpoint: &str,
        ) -> Result<CreateMessageResponse, ClaudeError> {
            let payload = ollama_adapter::anthropic_to_ollama_request(request);

            let resp = self
                .http
                .post(endpoint)
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .header("Content-Type", "application/json")
                .json(&payload)
                .timeout(self.config.request_timeout)
                .send()
                .await
                .map_err(|e| {
                    ClaudeError::Other(format!(
                        "{} request failed: {}",
                        self.config.provider.name(),
                        e
                    ))
                })?;

            let status = resp.status();
            let text = resp.text().await.map_err(ClaudeError::Http)?;

            if !status.is_success() {
                return Err(self.parse_api_error(status.as_u16(), &text));
            }

            let parsed: Value = serde_json::from_str(&text).map_err(ClaudeError::Json)?;
            let (content_blocks, stop_reason, input_tokens, output_tokens) =
                ollama_adapter::parse_ollama_response(&parsed);

            Ok(ollama_adapter::build_anthropic_response(
                content_blocks,
                &stop_reason,
                input_tokens,
                output_tokens,
                &request.model,
            ))
        }

        // ---- Streaming create message ------------------------------------

        /// Send a streaming `POST /v1/messages`.  Events are dispatched to the
        /// provided `handler` in real time, and also forwarded into the returned
        /// channel so the caller can drive a select loop.
        pub async fn create_message_stream(
            &self,
            mut request: CreateMessageRequest,
            handler: Arc<dyn StreamHandler>,
        ) -> Result<mpsc::Receiver<StreamEvent>, ClaudeError> {
            // Non-Anthropic providers don't have native streaming. Synthesize
            // a stream from a single non-streaming response so the rest of the
            // agent loop doesn't need special-casing.
            if self.config.provider != Provider::Anthropic {
                return self.synthesize_stream(request, handler).await;
            }

            request.stream = true;
            let body = serde_json::to_value(&request).map_err(ClaudeError::Json)?;

            let resp = self.send_with_retry(&body).await?;
            let status = resp.status();

            if !status.is_success() {
                let text = resp.text().await.map_err(ClaudeError::Http)?;
                return Err(self.parse_api_error(status.as_u16(), &text));
            }

            let (tx, rx) = mpsc::channel(256);

            // Spawn a task that reads the SSE byte stream and emits events.
            tokio::spawn(async move {
                if let Err(e) = Self::process_sse_stream(resp, handler, tx.clone()).await {
                    let _ = tx
                        .send(StreamEvent::Error {
                            error_type: "stream_error".into(),
                            message: e.to_string(),
                        })
                        .await;
                }
            });

            Ok(rx)
        }

        /// Fake a streaming response by calling create_message() and emitting
        /// the result as a single burst of stream events. Used for providers
        /// that don't support SSE (OpenAI-compat + Ollama + Codex).
        async fn synthesize_stream(
            &self,
            mut request: CreateMessageRequest,
            handler: Arc<dyn StreamHandler>,
        ) -> Result<mpsc::Receiver<StreamEvent>, ClaudeError> {
            // Force non-streaming: upstream sets stream=true but the
            // OpenAI-compat path expects a single JSON body, not SSE.
            request.stream = false;
            let response = self.create_message(request).await?;
            let (tx, rx) = mpsc::channel(64);

            let emit = |ev: StreamEvent,
                        handler: &Arc<dyn StreamHandler>,
                        tx: &mpsc::Sender<StreamEvent>| {
                handler.on_event(&ev);
                let _ = tx.try_send(ev);
            };

            emit(
                StreamEvent::MessageStart {
                    id: response.id.clone(),
                    model: response.model.clone(),
                    usage: response.usage.clone(),
                },
                &handler,
                &tx,
            );

            for (index, block) in response.content.iter().enumerate() {
                let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("text");
                match block_type {
                    "text" => {
                        let text = block
                            .get("text")
                            .and_then(|t| t.as_str())
                            .unwrap_or("")
                            .to_string();
                        emit(
                            StreamEvent::ContentBlockStart {
                                index,
                                content_block: ContentBlock::Text { text: String::new() },
                            },
                            &handler,
                            &tx,
                        );
                        emit(
                            StreamEvent::ContentBlockDelta {
                                index,
                                delta: ContentDelta::TextDelta { text },
                            },
                            &handler,
                            &tx,
                        );
                        emit(StreamEvent::ContentBlockStop { index }, &handler, &tx);
                    }
                    "tool_use" => {
                        let id = block
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = block
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let input = block.get("input").cloned().unwrap_or(Value::Null);
                        let partial_json = serde_json::to_string(&input).unwrap_or_default();
                        emit(
                            StreamEvent::ContentBlockStart {
                                index,
                                content_block: ContentBlock::ToolUse {
                                    id,
                                    name,
                                    input: Value::Null,
                                },
                            },
                            &handler,
                            &tx,
                        );
                        emit(
                            StreamEvent::ContentBlockDelta {
                                index,
                                delta: ContentDelta::InputJsonDelta { partial_json },
                            },
                            &handler,
                            &tx,
                        );
                        emit(StreamEvent::ContentBlockStop { index }, &handler, &tx);
                    }
                    _ => {
                        // Unknown block type — skip it but keep the indices.
                    }
                }
            }

            emit(
                StreamEvent::MessageDelta {
                    stop_reason: response.stop_reason.clone(),
                    usage: Some(response.usage.clone()),
                },
                &handler,
                &tx,
            );
            emit(StreamEvent::MessageStop, &handler, &tx);

            drop(tx);
            Ok(rx)
        }

        // ---- Models list ------------------------------------------------

        /// Fetch available models from `GET /v1/models`.
        ///
        /// Returns a list of models the current API key has access to.
        /// Falls back gracefully: returns an empty `Vec` on any error so
        /// callers can fall back to the hardcoded default list instead of
        /// surfacing an error.
        pub async fn fetch_available_models(&self) -> anyhow::Result<Vec<crate::AvailableModel>> {
            let url = format!("{}/v1/models", self.config.api_base);

            let mut req = self
                .http
                .get(&url)
                .header("anthropic-version", &self.config.api_version)
                .header("content-type", "application/json");
            req = if self.config.use_bearer_auth {
                req.header("Authorization", format!("Bearer {}", &self.config.api_key))
            } else {
                req.header("x-api-key", &self.config.api_key)
            };

            let resp = req.send().await?;

            if !resp.status().is_success() {
                anyhow::bail!("models endpoint returned {}", resp.status());
            }

            #[derive(serde::Deserialize)]
            struct ModelsResponse {
                data: Vec<crate::AvailableModel>,
            }

            let body: ModelsResponse = resp.json().await?;
            Ok(body.data)
        }

        // ---- Internal helpers --------------------------------------------

        /// Build the common request and execute with retry logic.
        async fn send_with_retry(
            &self,
            body: &Value,
        ) -> Result<reqwest::Response, ClaudeError> {
            let url = format!("{}/v1/messages", self.config.api_base);
            let mut attempts = 0u32;
            let mut delay = self.config.initial_retry_delay;

            // Serialize body to JSON string for CCH signing
            let body_str = serde_json::to_string(body)
                .map_err(|e| ClaudeError::Api(format!("Failed to serialize request: {}", e)))?;
            let body_bytes = body_str.as_bytes();

            loop {
                attempts += 1;

                // Compute CCH hash and build billing header
                let cch_hash = cch::compute_cch(body_bytes);
                let billing_header = format!("cc_version=0.1; cc_entrypoint=claude_code; {}; cc_workload=claude_code;", cch_hash);

                // Use Bearer auth for Claude.ai OAuth tokens; x-api-key for regular keys.
                let mut req = self
                    .http
                    .post(&url)
                    .header("anthropic-version", &self.config.api_version)
                    .header("anthropic-beta", &self.config.beta_features)
                    .header("content-type", "application/json")
                    .header("accept", "text/event-stream")
                    .header("x-anthropic-billing-header", billing_header);
                req = if self.config.use_bearer_auth {
                    req.header("Authorization", format!("Bearer {}", &self.config.api_key))
                } else {
                    req.header("x-api-key", &self.config.api_key)
                };
                let req = req.body(body_str.clone());

                let resp = req.send().await.map_err(ClaudeError::Http)?;
                let status = resp.status().as_u16();

                // 200-299: success
                if resp.status().is_success() {
                    return Ok(resp);
                }

                // 429 (rate limit) or 529 (overloaded): retry
                if (status == 429 || status == 529) && attempts <= self.config.max_retries {
                    // Honour Retry-After header if present
                    let retry_after = resp
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .map(Duration::from_secs);

                    let wait = retry_after.unwrap_or(delay);
                    warn!(
                        status,
                        attempt = attempts,
                        wait_secs = wait.as_secs(),
                        "Retryable API error, backing off"
                    );
                    tokio::time::sleep(wait).await;
                    delay = (delay * 2).min(self.config.max_retry_delay);
                    continue;
                }

                // Non-retryable error – return immediately
                let text = resp.text().await.unwrap_or_default();
                return Err(self.parse_api_error(status, &text));
            }
        }

        /// Parse an API error body into a typed `ClaudeError`.
        fn parse_api_error(&self, status: u16, body: &str) -> ClaudeError {
            if let Ok(err) = serde_json::from_str::<ApiErrorResponse>(body) {
                match status {
                    401 => ClaudeError::Auth(err.error.message),
                    429 => ClaudeError::RateLimit,
                    529 => ClaudeError::ApiStatus {
                        status,
                        message: format!("Overloaded: {}", err.error.message),
                    },
                    _ => ClaudeError::ApiStatus {
                        status,
                        message: err.error.message,
                    },
                }
            } else {
                ClaudeError::ApiStatus {
                    status,
                    message: body.to_string(),
                }
            }
        }

        /// Read an SSE byte stream, parse frames, and emit `StreamEvent`s.
        async fn process_sse_stream(
            resp: reqwest::Response,
            handler: Arc<dyn StreamHandler>,
            tx: mpsc::Sender<StreamEvent>,
        ) -> Result<(), ClaudeError> {
            use sse_parser::SseLineParser;

            let mut parser = SseLineParser::new();
            let mut byte_stream = resp.bytes_stream();
            let mut leftover = String::new();

            while let Some(chunk_result) = byte_stream.next().await {
                let chunk = chunk_result.map_err(ClaudeError::Http)?;
                let text = String::from_utf8_lossy(&chunk);

                // Prepend any leftover from the previous chunk
                let combined = if leftover.is_empty() {
                    text.to_string()
                } else {
                    let mut s = std::mem::take(&mut leftover);
                    s.push_str(&text);
                    s
                };

                // Split into lines.  If the chunk doesn't end with a newline
                // the last piece is an incomplete line – stash it.
                let mut lines: Vec<&str> = combined.split('\n').collect();
                if !combined.ends_with('\n') {
                    leftover = lines.pop().unwrap_or("").to_string();
                }

                for line in lines {
                    let line = line.trim_end_matches('\r');
                    if let Some(frame) = parser.feed_line(line) {
                        if let Some(event) =
                            Self::frame_to_event(&frame.event, &frame.data)
                        {
                            handler.on_event(&event);
                            if tx.send(event).await.is_err() {
                                // Receiver dropped – stop reading.
                                return Ok(());
                            }
                        }
                    }
                }
            }

            Ok(())
        }

        /// Convert a parsed SSE frame into a typed `StreamEvent`.
        fn frame_to_event(
            event_type: &Option<String>,
            data: &str,
        ) -> Option<StreamEvent> {
            let event_name = event_type.as_deref().unwrap_or("");

            match event_name {
                "ping" => Some(StreamEvent::Ping),

                "message_start" => {
                    let v: Value = serde_json::from_str(data).ok()?;
                    let msg = v.get("message")?;
                    let id = msg.get("id")?.as_str()?.to_string();
                    let model = msg.get("model")?.as_str()?.to_string();
                    let usage = msg
                        .get("usage")
                        .and_then(|u| serde_json::from_value::<UsageInfo>(u.clone()).ok())
                        .unwrap_or_default();

                    Some(StreamEvent::MessageStart { id, model, usage })
                }

                "content_block_start" => {
                    let v: Value = serde_json::from_str(data).ok()?;
                    let index = v.get("index")?.as_u64()? as usize;
                    let block_value = v.get("content_block")?;
                    let content_block: ContentBlock =
                        serde_json::from_value(block_value.clone()).ok()?;
                    Some(StreamEvent::ContentBlockStart {
                        index,
                        content_block,
                    })
                }

                "content_block_delta" => {
                    let v: Value = serde_json::from_str(data).ok()?;
                    let index = v.get("index")?.as_u64()? as usize;
                    let delta_value = v.get("delta")?;
                    let delta: streaming::ContentDelta =
                        serde_json::from_value(delta_value.clone()).ok()?;
                    Some(StreamEvent::ContentBlockDelta { index, delta })
                }

                "content_block_stop" => {
                    let v: Value = serde_json::from_str(data).ok()?;
                    let index = v.get("index")?.as_u64()? as usize;
                    Some(StreamEvent::ContentBlockStop { index })
                }

                "message_delta" => {
                    let v: Value = serde_json::from_str(data).ok()?;
                    let delta = v.get("delta")?;
                    let stop_reason = delta
                        .get("stop_reason")
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string());
                    let usage = v
                        .get("usage")
                        .and_then(|u| serde_json::from_value::<UsageInfo>(u.clone()).ok());
                    Some(StreamEvent::MessageDelta { stop_reason, usage })
                }

                "message_stop" => Some(StreamEvent::MessageStop),

                "error" => {
                    let v: Value = serde_json::from_str(data).ok()?;
                    let error = v.get("error")?;
                    let error_type = error
                        .get("type")
                        .and_then(|s| s.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let message = error
                        .get("message")
                        .and_then(|s| s.as_str())
                        .unwrap_or("Unknown error")
                        .to_string();
                    Some(StreamEvent::Error {
                        error_type,
                        message,
                    })
                }

                _ => {
                    debug!(event = event_name, "Unhandled SSE event type");
                    None
                }
            }
        }
    }

    #[cfg(test)]
    mod provider_tests {
        use super::Provider;

        // ---- parse_name: canonical names ------------------------------------

        #[test]
        fn parse_name_canonical_anthropic() {
            assert_eq!(Provider::parse_name("anthropic"), Some(Provider::Anthropic));
        }

        #[test]
        fn parse_name_canonical_codex() {
            assert_eq!(Provider::parse_name("codex"), Some(Provider::Codex));
        }

        #[test]
        fn parse_name_canonical_ollama() {
            assert_eq!(Provider::parse_name("ollama"), Some(Provider::Ollama));
        }

        #[test]
        fn parse_name_canonical_openai() {
            assert_eq!(Provider::parse_name("openai"), Some(Provider::OpenAI));
        }

        #[test]
        fn parse_name_canonical_gemini() {
            assert_eq!(Provider::parse_name("gemini"), Some(Provider::Gemini));
        }

        #[test]
        fn parse_name_canonical_zai() {
            assert_eq!(Provider::parse_name("zai"), Some(Provider::Zai));
        }

        #[test]
        fn parse_name_canonical_moonshot() {
            assert_eq!(Provider::parse_name("moonshot"), Some(Provider::Moonshot));
        }

        #[test]
        fn parse_name_canonical_alibaba() {
            assert_eq!(Provider::parse_name("alibaba"), Some(Provider::Alibaba));
        }

        #[test]
        fn parse_name_canonical_deepseek() {
            assert_eq!(Provider::parse_name("deepseek"), Some(Provider::DeepSeek));
        }

        // ---- parse_name: aliases -------------------------------------------

        #[test]
        fn parse_name_alias_claude() {
            assert_eq!(Provider::parse_name("claude"), Some(Provider::Anthropic));
        }

        #[test]
        fn parse_name_alias_local() {
            assert_eq!(Provider::parse_name("local"), Some(Provider::Ollama));
        }

        #[test]
        fn parse_name_alias_chatgpt() {
            assert_eq!(Provider::parse_name("chatgpt"), Some(Provider::OpenAI));
        }

        #[test]
        fn parse_name_alias_gpt() {
            assert_eq!(Provider::parse_name("gpt"), Some(Provider::OpenAI));
        }

        #[test]
        fn parse_name_alias_google() {
            assert_eq!(Provider::parse_name("google"), Some(Provider::Gemini));
        }

        #[test]
        fn parse_name_alias_glm() {
            assert_eq!(Provider::parse_name("glm"), Some(Provider::Zai));
        }

        #[test]
        fn parse_name_alias_zhipu() {
            assert_eq!(Provider::parse_name("zhipu"), Some(Provider::Zai));
        }

        #[test]
        fn parse_name_alias_bigmodel() {
            assert_eq!(Provider::parse_name("bigmodel"), Some(Provider::Zai));
        }

        #[test]
        fn parse_name_alias_kimi() {
            assert_eq!(Provider::parse_name("kimi"), Some(Provider::Moonshot));
        }

        #[test]
        fn parse_name_alias_qwen() {
            assert_eq!(Provider::parse_name("qwen"), Some(Provider::Alibaba));
        }

        #[test]
        fn parse_name_alias_dashscope() {
            assert_eq!(Provider::parse_name("dashscope"), Some(Provider::Alibaba));
        }

        // ---- parse_name: case insensitivity and whitespace ------------------

        #[test]
        fn parse_name_case_insensitive_upper() {
            assert_eq!(Provider::parse_name("ANTHROPIC"), Some(Provider::Anthropic));
        }

        #[test]
        fn parse_name_case_insensitive_mixed() {
            assert_eq!(Provider::parse_name("DeepSeek"), Some(Provider::DeepSeek));
        }

        #[test]
        fn parse_name_trims_whitespace() {
            assert_eq!(Provider::parse_name("  gemini  "), Some(Provider::Gemini));
        }

        // ---- parse_name: invalid returns None ------------------------------

        #[test]
        fn parse_name_invalid_returns_none() {
            assert_eq!(Provider::parse_name("unknown_provider"), None);
        }

        #[test]
        fn parse_name_empty_returns_none() {
            assert_eq!(Provider::parse_name(""), None);
        }

        #[test]
        fn parse_name_whitespace_only_returns_none() {
            assert_eq!(Provider::parse_name("   "), None);
        }

        // ---- from_model: one case per provider prefix ----------------------

        #[test]
        fn from_model_claude_prefix() {
            assert_eq!(Provider::from_model("claude-3-5-sonnet"), Provider::Anthropic);
        }

        #[test]
        fn from_model_anthropic_prefix() {
            assert_eq!(Provider::from_model("anthropic-model"), Provider::Anthropic);
        }

        #[test]
        fn from_model_gpt_prefix() {
            assert_eq!(Provider::from_model("gpt-4o"), Provider::OpenAI);
        }

        #[test]
        fn from_model_o1_prefix() {
            assert_eq!(Provider::from_model("o1-preview"), Provider::OpenAI);
        }

        #[test]
        fn from_model_o3_prefix() {
            assert_eq!(Provider::from_model("o3-mini"), Provider::OpenAI);
        }

        #[test]
        fn from_model_o4_prefix() {
            assert_eq!(Provider::from_model("o4-mini"), Provider::OpenAI);
        }

        #[test]
        fn from_model_chatgpt_prefix() {
            assert_eq!(Provider::from_model("chatgpt-4o-latest"), Provider::OpenAI);
        }

        #[test]
        fn from_model_gemini_prefix() {
            assert_eq!(Provider::from_model("gemini-2.5-pro"), Provider::Gemini);
        }

        #[test]
        fn from_model_glm_prefix() {
            assert_eq!(Provider::from_model("glm-4-flash"), Provider::Zai);
        }

        #[test]
        fn from_model_kimi_prefix() {
            assert_eq!(Provider::from_model("kimi-k2"), Provider::Moonshot);
        }

        #[test]
        fn from_model_moonshot_prefix() {
            assert_eq!(Provider::from_model("moonshot-v1-8k"), Provider::Moonshot);
        }

        #[test]
        fn from_model_qwen_prefix() {
            assert_eq!(Provider::from_model("qwen3-max"), Provider::Alibaba);
        }

        #[test]
        fn from_model_deepseek_prefix() {
            assert_eq!(Provider::from_model("deepseek-v3"), Provider::DeepSeek);
        }

        #[test]
        fn from_model_empty_defaults_to_anthropic() {
            assert_eq!(Provider::from_model(""), Provider::Anthropic);
        }

        #[test]
        fn from_model_unknown_prefix_defaults_to_anthropic() {
            assert_eq!(Provider::from_model("llama3.3:8b"), Provider::Anthropic);
        }

        // ---- chat_completions_url ------------------------------------------

        #[test]
        fn chat_completions_url_openai_is_some_with_openai_domain() {
            let url = Provider::OpenAI.chat_completions_url().unwrap();
            assert!(url.contains("openai.com"), "expected openai.com in {url}");
        }

        #[test]
        fn chat_completions_url_gemini_contains_googleapis() {
            let url = Provider::Gemini.chat_completions_url().unwrap();
            assert!(
                url.contains("generativelanguage.googleapis.com"),
                "expected googleapis domain in {url}"
            );
        }

        #[test]
        fn chat_completions_url_zai_contains_bigmodel() {
            let url = Provider::Zai.chat_completions_url().unwrap();
            assert!(url.contains("bigmodel.cn"), "expected bigmodel.cn in {url}");
        }

        #[test]
        fn chat_completions_url_moonshot_contains_moonshot_ai() {
            let url = Provider::Moonshot.chat_completions_url().unwrap();
            assert!(url.contains("moonshot.ai"), "expected moonshot.ai in {url}");
        }

        #[test]
        fn chat_completions_url_alibaba_contains_dashscope() {
            let url = Provider::Alibaba.chat_completions_url().unwrap();
            assert!(
                url.contains("dashscope.aliyuncs.com"),
                "expected dashscope.aliyuncs.com in {url}"
            );
        }

        #[test]
        fn chat_completions_url_deepseek_contains_deepseek_com() {
            let url = Provider::DeepSeek.chat_completions_url().unwrap();
            assert!(url.contains("deepseek.com"), "expected deepseek.com in {url}");
        }

        #[test]
        fn chat_completions_url_anthropic_is_none() {
            assert!(Provider::Anthropic.chat_completions_url().is_none());
        }

        #[test]
        fn chat_completions_url_codex_is_none() {
            assert!(Provider::Codex.chat_completions_url().is_none());
        }

        #[test]
        fn chat_completions_url_ollama_is_none() {
            assert!(Provider::Ollama.chat_completions_url().is_none());
        }

        // ---- api_key_env ---------------------------------------------------

        #[test]
        fn api_key_env_anthropic() {
            assert_eq!(Provider::Anthropic.api_key_env(), "ANTHROPIC_API_KEY");
        }

        #[test]
        fn api_key_env_openai() {
            assert_eq!(Provider::OpenAI.api_key_env(), "OPENAI_API_KEY");
        }

        #[test]
        fn api_key_env_gemini() {
            assert_eq!(Provider::Gemini.api_key_env(), "GEMINI_API_KEY");
        }

        #[test]
        fn api_key_env_zai() {
            assert_eq!(Provider::Zai.api_key_env(), "ZAI_API_KEY");
        }

        #[test]
        fn api_key_env_moonshot() {
            assert_eq!(Provider::Moonshot.api_key_env(), "MOONSHOT_API_KEY");
        }

        #[test]
        fn api_key_env_alibaba() {
            assert_eq!(Provider::Alibaba.api_key_env(), "DASHSCOPE_API_KEY");
        }

        #[test]
        fn api_key_env_deepseek() {
            assert_eq!(Provider::DeepSeek.api_key_env(), "DEEPSEEK_API_KEY");
        }

        #[test]
        fn api_key_env_ollama() {
            assert_eq!(Provider::Ollama.api_key_env(), "OLLAMA_API_KEY");
        }

        #[test]
        fn api_key_env_codex() {
            assert_eq!(Provider::Codex.api_key_env(), "CODEX_API_KEY");
        }

        // ---- is_openai_compat ----------------------------------------------

        #[test]
        fn is_openai_compat_true_for_openai() {
            assert!(Provider::OpenAI.is_openai_compat());
        }

        #[test]
        fn is_openai_compat_true_for_gemini() {
            assert!(Provider::Gemini.is_openai_compat());
        }

        #[test]
        fn is_openai_compat_true_for_zai() {
            assert!(Provider::Zai.is_openai_compat());
        }

        #[test]
        fn is_openai_compat_true_for_moonshot() {
            assert!(Provider::Moonshot.is_openai_compat());
        }

        #[test]
        fn is_openai_compat_true_for_alibaba() {
            assert!(Provider::Alibaba.is_openai_compat());
        }

        #[test]
        fn is_openai_compat_true_for_deepseek() {
            assert!(Provider::DeepSeek.is_openai_compat());
        }

        #[test]
        fn is_openai_compat_false_for_anthropic() {
            assert!(!Provider::Anthropic.is_openai_compat());
        }

        #[test]
        fn is_openai_compat_false_for_codex() {
            assert!(!Provider::Codex.is_openai_compat());
        }

        #[test]
        fn is_openai_compat_false_for_ollama() {
            assert!(!Provider::Ollama.is_openai_compat());
        }
    }
}

// ---------------------------------------------------------------------------
// Convenience builder for CreateMessageRequest
// ---------------------------------------------------------------------------

impl CreateMessageRequest {
    /// Create a minimal request builder.
    pub fn builder(model: impl Into<String>, max_tokens: u32) -> CreateMessageRequestBuilder {
        CreateMessageRequestBuilder {
            model: model.into(),
            max_tokens,
            messages: vec![],
            system: None,
            tools: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            thinking: None,
        }
    }
}

pub struct CreateMessageRequestBuilder {
    model: String,
    max_tokens: u32,
    messages: Vec<ApiMessage>,
    system: Option<SystemPrompt>,
    tools: Option<Vec<ApiToolDefinition>>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
    stop_sequences: Option<Vec<String>>,
    thinking: Option<ThinkingConfig>,
}

impl CreateMessageRequestBuilder {
    pub fn messages(mut self, msgs: Vec<ApiMessage>) -> Self {
        self.messages = msgs;
        self
    }

    pub fn add_message(mut self, msg: ApiMessage) -> Self {
        self.messages.push(msg);
        self
    }

    pub fn system(mut self, s: SystemPrompt) -> Self {
        self.system = Some(s);
        self
    }

    pub fn system_text(mut self, text: impl Into<String>) -> Self {
        self.system = Some(SystemPrompt::Text(text.into()));
        self
    }

    pub fn tools(mut self, tools: Vec<ApiToolDefinition>) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn temperature(mut self, t: f32) -> Self {
        self.temperature = Some(t);
        self
    }

    pub fn top_p(mut self, p: f32) -> Self {
        self.top_p = Some(p);
        self
    }

    pub fn top_k(mut self, k: u32) -> Self {
        self.top_k = Some(k);
        self
    }

    pub fn stop_sequences(mut self, seqs: Vec<String>) -> Self {
        self.stop_sequences = Some(seqs);
        self
    }

    pub fn thinking(mut self, config: ThinkingConfig) -> Self {
        self.thinking = Some(config);
        self
    }

    pub fn build(self) -> CreateMessageRequest {
        CreateMessageRequest {
            model: self.model,
            max_tokens: self.max_tokens,
            messages: self.messages,
            system: self.system,
            tools: self.tools,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            stop_sequences: self.stop_sequences,
            stream: true,
            thinking: self.thinking,
        }
    }
}

// ---------------------------------------------------------------------------
// Accumulated message builder – reconstructs a full Message from stream events
// ---------------------------------------------------------------------------

/// Collects streaming events and produces a finished `Message` plus usage info.
pub struct StreamAccumulator {
    id: Option<String>,
    model: Option<String>,
    content_blocks: Vec<ContentBlock>,
    /// Partial accumulators keyed by block index.
    partials: std::collections::HashMap<usize, PartialBlock>,
    stop_reason: Option<String>,
    usage: UsageInfo,
}

#[derive(Debug)]
enum PartialBlock {
    Text(String),
    ToolUse {
        id: String,
        name: String,
        json_buf: String,
    },
    Thinking {
        thinking_buf: String,
        signature_buf: String,
    },
}

impl StreamAccumulator {
    pub fn new() -> Self {
        Self {
            id: None,
            model: None,
            content_blocks: vec![],
            partials: Default::default(),
            stop_reason: None,
            usage: UsageInfo::default(),
        }
    }

    /// Feed a stream event. Call this for every event received from the stream.
    pub fn on_event(&mut self, event: &StreamEvent) {
        match event {
            StreamEvent::MessageStart { id, model, usage } => {
                self.id = Some(id.clone());
                self.model = Some(model.clone());
                self.usage = usage.clone();
            }

            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                let partial = match content_block {
                    ContentBlock::Text { text } => PartialBlock::Text(text.clone()),
                    ContentBlock::ToolUse { id, name, .. } => PartialBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        json_buf: String::new(),
                    },
                    ContentBlock::Thinking { thinking, signature } => PartialBlock::Thinking {
                        thinking_buf: thinking.clone(),
                        signature_buf: signature.clone(),
                    },
                    _ => return,
                };
                self.partials.insert(*index, partial);
            }

            StreamEvent::ContentBlockDelta { index, delta } => {
                if let Some(partial) = self.partials.get_mut(index) {
                    match (partial, delta) {
                        (PartialBlock::Text(buf), streaming::ContentDelta::TextDelta { text }) => {
                            buf.push_str(text);
                        }
                        (
                            PartialBlock::ToolUse { json_buf, .. },
                            streaming::ContentDelta::InputJsonDelta { partial_json },
                        ) => {
                            json_buf.push_str(partial_json);
                        }
                        (
                            PartialBlock::Thinking { thinking_buf, .. },
                            streaming::ContentDelta::ThinkingDelta { thinking },
                        ) => {
                            thinking_buf.push_str(thinking);
                        }
                        (
                            PartialBlock::Thinking { signature_buf, .. },
                            streaming::ContentDelta::SignatureDelta { signature },
                        ) => {
                            signature_buf.push_str(signature);
                        }
                        _ => {}
                    }
                }
            }

            StreamEvent::ContentBlockStop { index } => {
                if let Some(partial) = self.partials.remove(index) {
                    let block = match partial {
                        PartialBlock::Text(text) => ContentBlock::Text { text },
                        PartialBlock::ToolUse { id, name, json_buf } => {
                            let input = serde_json::from_str(&json_buf)
                                .unwrap_or(Value::Object(Default::default()));
                            ContentBlock::ToolUse { id, name, input }
                        }
                        PartialBlock::Thinking {
                            thinking_buf,
                            signature_buf,
                        } => ContentBlock::Thinking {
                            thinking: thinking_buf,
                            signature: signature_buf,
                        },
                    };
                    self.content_blocks.push(block);
                }
            }

            StreamEvent::MessageDelta { stop_reason, usage } => {
                if let Some(sr) = stop_reason {
                    self.stop_reason = Some(sr.clone());
                }
                if let Some(u) = usage {
                    // The delta usage usually only has output_tokens;
                    // add them to the running total.
                    self.usage.output_tokens += u.output_tokens;
                }
            }

            StreamEvent::MessageStop => {}
            StreamEvent::Ping => {}
            StreamEvent::Error { .. } => {}
        }
    }

    /// Finalize and produce the accumulated `Message`.
    pub fn finish(self) -> (Message, UsageInfo, Option<String>) {
        let msg = Message::assistant_blocks(self.content_blocks);
        (msg, self.usage, self.stop_reason)
    }

    pub fn stop_reason(&self) -> Option<&str> {
        self.stop_reason.as_deref()
    }

    pub fn usage(&self) -> &UsageInfo {
        &self.usage
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_parser_basic() {
        let mut parser = sse_parser::SseLineParser::new();
        assert!(parser.feed_line("event: message_start").is_none());
        assert!(parser
            .feed_line(r#"data: {"message":{"id":"m1","model":"claude","usage":{"input_tokens":0,"output_tokens":0}}}"#)
            .is_none());
        let frame = parser.feed_line("").expect("should produce frame");
        assert_eq!(frame.event.as_deref(), Some("message_start"));
        assert!(frame.data.contains("m1"));
    }

    #[test]
    fn test_create_message_request_builder() {
        let req = CreateMessageRequest::builder("claude-opus-4-6", 4096)
            .system_text("You are helpful.")
            .temperature(0.7)
            .build();
        assert_eq!(req.model, "claude-opus-4-6");
        assert_eq!(req.max_tokens, 4096);
        assert!(req.stream);
    }

    #[test]
    fn test_stream_accumulator_text() {
        let mut acc = StreamAccumulator::new();
        acc.on_event(&StreamEvent::MessageStart {
            id: "m1".into(),
            model: "claude".into(),
            usage: UsageInfo::default(),
        });
        acc.on_event(&StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Text {
                text: String::new(),
            },
        });
        acc.on_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: streaming::ContentDelta::TextDelta {
                text: "Hello ".into(),
            },
        });
        acc.on_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: streaming::ContentDelta::TextDelta {
                text: "world!".into(),
            },
        });
        acc.on_event(&StreamEvent::ContentBlockStop { index: 0 });
        acc.on_event(&StreamEvent::MessageDelta {
            stop_reason: Some("end_turn".into()),
            usage: None,
        });
        acc.on_event(&StreamEvent::MessageStop);

        let (msg, _usage, stop) = acc.finish();
        assert_eq!(msg.get_text(), Some("Hello world!"));
        assert_eq!(stop.as_deref(), Some("end_turn"));
    }
}
