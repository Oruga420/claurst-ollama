# GWS Tool — Google Workspace CLI Integration

## Overview

The GWS tool (`crates/tools/src/gws_tool.rs`) wraps the [Google Workspace CLI](https://github.com/googleworkspace/cli) (`gws`) to give the LLM direct access to Google Workspace services.

## Prerequisites

Install the GWS CLI:
```bash
# Via npm
npm install -g @googleworkspace/cli

# Authenticate
gws auth login    # Opens browser for Google OAuth
```

## What the LLM Can Do

When the model receives a user request like "schedule a meeting tomorrow at 3pm", it can call the `gws` tool with the appropriate command. The tool executes the command and returns the result.

## Service Reference

### Calendar

```bash
# List upcoming events
gws calendar +agenda

# List events with params
gws calendar events list --params '{"calendarId":"primary","maxResults":10,"timeMin":"2026-04-08T00:00:00Z","orderBy":"startTime","singleEvents":true}'

# Create event
gws calendar events insert --params '{"calendarId":"primary"}' --json '{"summary":"Team Meeting","start":{"dateTime":"2026-04-09T15:00:00","timeZone":"America/Toronto"},"end":{"dateTime":"2026-04-09T16:00:00","timeZone":"America/Toronto"}}'

# Delete event
gws calendar events delete --params '{"calendarId":"primary","eventId":"<id>"}'

# Get event by ID
gws calendar events get --params '{"calendarId":"primary","eventId":"<id>"}'
```

### Gmail

```bash
# Send email
gws gmail +send --to user@example.com --subject "Hello" --body "Message body"

# Triage inbox
gws gmail +triage

# List messages
gws gmail users messages list --params '{"userId":"me","maxResults":5}'

# Create draft
gws gmail users drafts create --params '{"userId":"me"}' --json '{"message":{"raw":"<base64>"}}'
```

### Drive

```bash
# Search files
gws drive files list --params '{"pageSize":10,"q":"name contains '\''report'\''"}'

# Upload file
gws drive +upload ./report.pdf
```

### Docs

```bash
# Create document
gws docs documents create --json '{"title":"Meeting Notes"}'

# Append text
gws docs +write --document-id <id> --text "Content to append"
```

### Sheets

```bash
# Read data
gws sheets +read --spreadsheet-id <id> --range "Sheet1!A1:D10"

# Append row
gws sheets +append --spreadsheet-id <id> --range "Sheet1" --values '[["value1","value2"]]'
```

### Tasks

```bash
# List task lists
gws tasks tasklists list

# List tasks
gws tasks tasks list --params '{"tasklist":"<id>","maxResults":20}'

# Create task
gws tasks tasks insert --params '{"tasklist":"<id>"}' --json '{"title":"Buy groceries","due":"2026-04-10T00:00:00.000Z"}'
```

### Workflow (Compound Commands)

```bash
# Standup report (today's meetings + open tasks)
gws workflow +standup-report

# Meeting prep (agenda, attendees, linked docs)
gws workflow +meeting-prep
```

### Introspection

```bash
# Get API schema for any method
gws schema calendar.events.insert

# Check auth status
gws auth status
```

## Output Format

All GWS output is **JSON by default**, which the LLM can parse and act on. Use `--format table` for human-readable output.

## Error Codes

| Exit Code | Meaning |
|-----------|---------|
| 0 | Success |
| 1 | API error (check stderr) |
| 2 | Auth error (token expired) |
| 3 | Validation error (bad params) |
| 4 | Discovery error |
| 5 | Internal error |

## Permission Level

The GWS tool has permission level `Execute` because it can modify external state (send emails, create events, delete files). The CLAURST permission system will prompt the user before executing unless auto-approve is configured.
