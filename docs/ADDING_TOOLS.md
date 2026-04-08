# Adding Custom Tools to CLAURST

## Overview

Every tool in CLAURST implements the `Tool` trait. The LLM sees the tool's name, description, and JSON schema — then decides when to call it.

## Step-by-Step

### 1. Create the file

Create `crates/tools/src/my_tool.rs`:

```rust
use crate::{PermissionLevel, Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

pub struct MyTool;

#[derive(Debug, Deserialize)]
struct MyInput {
    query: String,
    #[serde(default)]
    max_results: Option<u32>,
}

#[async_trait]
impl Tool for MyTool {
    fn name(&self) -> &str {
        "my_tool"
    }

    fn description(&self) -> &str {
        "Description the LLM sees. Be specific about what this tool does, \
         what inputs it expects, and what it returns."
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly  // or Execute, Write, etc.
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What to search for"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Max results to return",
                    "default": 10
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let parsed: MyInput = match serde_json::from_value(input) {
            Ok(v) => v,
            Err(e) => return ToolResult::error(format!("Bad input: {}", e)),
        };

        // Your logic here
        let result = format!("Found results for: {}", parsed.query);

        ToolResult::success(result)
    }
}
```

### 2. Register in lib.rs

In `crates/tools/src/lib.rs`:

```rust
// Add module
pub mod my_tool;

// Add re-export
pub use my_tool::MyTool;
```

### 3. Build and test

```bash
cargo check --package claurst-tools
```

## Permission Levels

| Level | When to Use |
|-------|------------|
| `None` | Tool has no side effects (rare) |
| `ReadOnly` | Only reads data (file read, search, web fetch) |
| `Write` | Modifies local files |
| `Execute` | Runs commands or modifies external state |
| `Dangerous` | Could cause damage (use sparingly) |
| `Forbidden` | Never allowed |

## Best Practices

1. **Description is everything** — The LLM decides to use your tool based solely on the description. Be explicit about what it does, include examples.

2. **Schema validates input** — Define JSON Schema precisely. Use `required` fields. Add `description` to every property.

3. **Return useful errors** — Use `ToolResult::error()` with actionable messages the LLM can reason about.

4. **Keep it focused** — One tool = one capability. Don't make a "swiss army knife" tool.

5. **Parse defensively** — Always handle `serde_json::from_value` errors.

## Example: Wrapping a CLI Tool

Most custom tools wrap an existing CLI. Pattern:

```rust
async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
    let parsed: MyInput = serde_json::from_value(input)
        .map_err(|e| return ToolResult::error(format!("{}", e)))?;

    let output = tokio::process::Command::new("my-cli")
        .arg(parsed.query)
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            ToolResult::success(String::from_utf8_lossy(&o.stdout))
        }
        Ok(o) => {
            ToolResult::error(format!(
                "Exit {}: {}",
                o.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&o.stderr)
            ))
        }
        Err(e) => ToolResult::error(format!("Failed: {}", e)),
    }
}
```
