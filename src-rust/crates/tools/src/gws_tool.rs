// GWS Tool: Execute Google Workspace CLI commands.
//
// This tool wraps the `gws` CLI to give the LLM direct access to Google
// Workspace services: Calendar, Gmail, Drive, Docs, Sheets, Tasks, etc.
//
// The LLM selects the appropriate gws subcommand and parameters based on
// the user's intent, and this tool executes it and returns the result.

use crate::{PermissionLevel, Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tracing::debug;

pub struct GwsTool;

#[derive(Debug, Deserialize)]
struct GwsInput {
    /// The full gws command to execute (without the 'gws' prefix).
    /// Examples:
    ///   "calendar events insert --params '{...}' --json '{...}'"
    ///   "gmail +send --to user@example.com --subject 'Hi' --body 'Hello'"
    ///   "calendar +agenda"
    ///   "tasks tasklists list"
    command: String,

    /// Human-readable description of what this command does.
    #[serde(default)]
    description: Option<String>,

    /// If true, do a dry-run (validate without executing).
    #[serde(default)]
    dry_run: bool,
}

#[async_trait]
impl Tool for GwsTool {
    fn name(&self) -> &str {
        "gws"
    }

    fn description(&self) -> &str {
        r#"Execute Google Workspace CLI commands. Access Calendar, Gmail, Drive, Docs, Sheets, Tasks, and more.

## Services & Commands

### Calendar
- `calendar +agenda` — List upcoming events
- `calendar +insert` — Create event (interactive)
- `calendar events list --params '{"calendarId":"primary","maxResults":10,"timeMin":"<ISO>","orderBy":"startTime","singleEvents":true}'`
- `calendar events insert --params '{"calendarId":"primary"}' --json '{"summary":"<title>","start":{"dateTime":"<ISO>","timeZone":"America/Toronto"},"end":{"dateTime":"<ISO>","timeZone":"America/Toronto"}}'`
- `calendar events delete --params '{"calendarId":"primary","eventId":"<id>"}'`
- `calendar events get --params '{"calendarId":"primary","eventId":"<id>"}'`

### Gmail
- `gmail +send --to <email> --subject "<subject>" --body "<body>"` — Send email
- `gmail +triage` — Triage unread inbox
- `gmail users messages list --params '{"userId":"me","maxResults":5}'`
- `gmail users drafts create --params '{"userId":"me"}' --json '<draft>'`

### Drive
- `drive files list --params '{"pageSize":10,"q":"name contains '\''<query>'\''"}'`
- `drive +upload <path>` — Upload file

### Docs
- `docs documents create --json '{"title":"<title>"}'`
- `docs +write --document-id <id> --text "<content>"`

### Sheets
- `sheets +read --spreadsheet-id <id> --range "<range>"`
- `sheets +append --spreadsheet-id <id> --range "Sheet1" --values '<json>'`

### Tasks
- `tasks tasklists list`
- `tasks tasks list --params '{"tasklist":"<id>","maxResults":20}'`
- `tasks tasks insert --params '{"tasklist":"<id>"}' --json '{"title":"<title>","due":"<ISO>"}'`

### Workflow (compound)
- `workflow +standup-report`
- `workflow +meeting-prep`

### Introspection
- `schema <service.resource.method>` — Get full API schema for any method
- `auth status` — Check auth status

## Output
All output is JSON by default. Use `--format table` for human-readable.
Add `--dry-run` to validate without executing."#
    }

    fn permission_level(&self) -> PermissionLevel {
        // GWS commands can modify external state (send emails, create events)
        PermissionLevel::Execute
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The gws command to execute (without the 'gws' prefix). Example: 'calendar events list --params {\"calendarId\":\"primary\",\"maxResults\":5}'"
                },
                "description": {
                    "type": "string",
                    "description": "Brief description of what this command does"
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "If true, validate the command without executing",
                    "default": false
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let parsed: GwsInput = match serde_json::from_value(input) {
            Ok(v) => v,
            Err(e) => {
                return ToolResult::error(format!("Invalid input: {}", e));
            }
        };

        let command = parsed.command.trim();
        if command.is_empty() {
            return ToolResult::error("Empty command. Provide a gws subcommand.");
        }

        // Build full command
        let mut full_cmd = format!("gws {}", command);
        if parsed.dry_run {
            full_cmd.push_str(" --dry-run");
        }

        debug!("GWS executing: {}", full_cmd);

        // Execute via shell
        let result = Command::new("bash")
            .arg("-c")
            .arg(&full_cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .output()
            .await;

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                if output.status.success() {
                    let mut result_text = stdout;
                    // Include stderr if it has useful info (warnings, etc.)
                    if !stderr.is_empty() && !stderr.contains("keyring") {
                        result_text.push_str("\n[stderr] ");
                        result_text.push_str(&stderr);
                    }
                    ToolResult::success(if result_text.trim().is_empty() {
                        "Command executed successfully (no output).".to_string()
                    } else {
                        result_text
                    })
                } else {
                    let exit_code = output.status.code().unwrap_or(-1);
                    ToolResult::error(format!(
                        "GWS command failed (exit {})\nstdout: {}\nstderr: {}",
                        exit_code,
                        stdout.trim(),
                        stderr.trim()
                    ))
                }
            }
            Err(e) => ToolResult::error(format!("Failed to execute gws: {}. Is gws installed?", e)),
        }
    }
}
