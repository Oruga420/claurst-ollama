//! Bundled skill definitions for the Skill tool.
//!
//! Each entry in `BUNDLED_SKILLS` mirrors one of the TypeScript
//! `registerXxxSkill()` calls under `src/skills/bundled/`.  Only publicly
//! invocable, user-facing skills are included; internal or ANT-only skills
//! (stuck, remember, verify) are omitted from the user-visible list but are
//! still present as documentation stubs so callers can discover them.
//!
//! The `SkillTool` checks bundled skills *before* scanning disk directories,
//! so bundled names take precedence over same-named `.md` files.

/// A single bundled skill definition.
#[derive(Debug, Clone)]
pub struct BundledSkill {
    /// Primary name used to invoke the skill (e.g. `"simplify"`).
    pub name: &'static str,
    /// One-line description shown in `/skill list` output and to the model.
    pub description: &'static str,
    /// Additional names that map to this skill.
    pub aliases: &'static [&'static str],
    /// Optional guidance for the model about when to auto-invoke.
    pub when_to_use: Option<&'static str>,
    /// Placeholder shown next to the skill name in help text.
    pub argument_hint: Option<&'static str>,
    /// The prompt template.  `$ARGUMENTS` is replaced at call time.
    /// `$ARGUMENTS_SUFFIX` expands to `": <args>"` when args are non-empty,
    /// or `""` otherwise.
    pub prompt_template: &'static str,
    /// If `Some`, only these tool names are available during the skill run.
    pub allowed_tools: Option<&'static [&'static str]>,
    /// Whether a human user can invoke this skill via `/skill <name>`.
    pub user_invocable: bool,
}

/// All bundled skills.
pub const BUNDLED_SKILLS: &[BundledSkill] = &[
    // -----------------------------------------------------------------------
    // simplify
    // -----------------------------------------------------------------------
    BundledSkill {
        name: "simplify",
        description: "Review changed code for reuse, quality, and efficiency, then fix any issues found.",
        aliases: &[],
        when_to_use: Some("After writing code, when you want a quality review and cleanup pass."),
        argument_hint: None,
        prompt_template: r#"# Simplify: Code Review and Cleanup

Review all changed files for reuse, quality, and efficiency. Fix any issues found.

## Phase 1: Identify Changes

Run `git diff` (or `git diff HEAD` if there are staged changes) to see what changed.
If there are no git changes, review the most recently modified files that were
mentioned or edited earlier in this conversation.

## Phase 2: Launch Three Review Agents in Parallel

Use the Agent tool to launch all three agents concurrently in a single message.
Pass each agent the full diff so it has complete context.

### Agent 1: Code Reuse Review

For each change:
1. **Search for existing utilities and helpers** that could replace newly written code.
2. **Flag any new function that duplicates existing functionality.**
3. **Flag any inline logic that could use an existing utility** — hand-rolled string
   manipulation, manual path handling, custom environment checks, etc.

### Agent 2: Code Quality Review

Review the same changes for hacky patterns:
1. **Redundant state** that duplicates existing state.
2. **Parameter sprawl** — new parameters instead of restructuring.
3. **Copy-paste with slight variation** that should be unified.
4. **Leaky abstractions** — exposing internal details.
5. **Stringly-typed code** where constants or enums already exist.
6. **Unnecessary comments** narrating what code does (not why).

### Agent 3: Efficiency Review

Review the same changes for efficiency:
1. **Unnecessary work** — redundant computations, duplicate reads.
2. **Missed concurrency** — independent operations run sequentially.
3. **Hot-path bloat** — blocking work added to startup or per-request paths.
4. **Recurring no-op updates** — unconditional updates in polling loops.
5. **Memory** — unbounded data structures, missing cleanup.

## Phase 3: Fix Issues

Wait for all three agents to complete. Aggregate findings and fix each issue.
If a finding is a false positive, note it and move on.

When done, briefly summarize what was fixed (or confirm the code was already clean).
$ARGUMENTS_SUFFIX"#,
        allowed_tools: None,
        user_invocable: true,
    },

    // -----------------------------------------------------------------------
    // remember
    // -----------------------------------------------------------------------
    BundledSkill {
        name: "remember",
        description: "Review auto-memory entries and propose promotions to CLAUDE.md, CLAUDE.local.md, or shared memory.",
        aliases: &["mem", "save"],
        when_to_use: Some("When the user wants to review, organise, or promote their auto-memory entries."),
        argument_hint: Some("[additional context]"),
        prompt_template: r#"# Memory Review

## Goal
Review the user's memory landscape and produce a clear report of proposed changes,
grouped by action type. Do NOT apply changes — present proposals for user approval.

## Steps

### 1. Gather all memory layers
Read CLAUDE.md and CLAUDE.local.md from the project root (if they exist).
Your auto-memory content is already in your system prompt — review it there.

### 2. Classify each auto-memory entry

| Destination | What belongs there |
|---|---|
| **CLAUDE.md** | Project conventions all contributors should follow |
| **CLAUDE.local.md** | Personal instructions specific to this user |
| **Stay in auto-memory** | Working notes, temporary context, uncertain patterns |

### 3. Identify cleanup opportunities
- **Duplicates**: auto-memory entries already in CLAUDE.md → propose removing
- **Outdated**: CLAUDE.md entries contradicted by newer auto-memory → propose updating
- **Conflicts**: contradictions between layers → propose resolution

### 4. Present the report
Output a structured report grouped by: Promotions, Cleanup, Ambiguous, No action needed.

## Rules
- Present ALL proposals before making any changes
- Do NOT modify files without explicit user approval
- Ask about ambiguous entries — don't guess
$ARGUMENTS_SUFFIX"#,
        allowed_tools: Some(&["Read", "Write", "Edit", "Glob"]),
        user_invocable: true,
    },

    // -----------------------------------------------------------------------
    // debug
    // -----------------------------------------------------------------------
    BundledSkill {
        name: "debug",
        description: "Enable debug logging for this session and help diagnose issues.",
        aliases: &["diagnose"],
        when_to_use: Some("When there is an error, bug, or unexpected behaviour to investigate."),
        argument_hint: Some("[issue description or error message]"),
        prompt_template: r#"# Debug Skill

Help the user debug an issue they are encountering.

## Issue Description

$ARGUMENTS

## Systematic Debugging Approach

1. **Reproduce** — Confirm the exact error / behaviour.
2. **Locate** — Find the relevant code (read files, grep for error messages).
3. **Hypothesize** — Form 2–3 hypotheses about the root cause.
4. **Test** — Verify each hypothesis systematically.
5. **Fix** — Implement the fix for the confirmed root cause.
6. **Verify** — Confirm the fix resolves the issue.

## Settings Reference

Settings files are in:
- User:    ~/.claude/settings.json
- Project: .claude/settings.json
- Local:   .claude/settings.local.json

Read the relevant files before making any changes."#,
        allowed_tools: Some(&["Read", "Grep", "Glob"]),
        user_invocable: true,
    },

    // -----------------------------------------------------------------------
    // stuck
    // -----------------------------------------------------------------------
    BundledSkill {
        name: "stuck",
        description: "Help get unstuck when you don't know how to proceed.",
        aliases: &["help-me", "unblock"],
        when_to_use: Some("When you are stuck, confused, or don't know how to proceed."),
        argument_hint: Some("[what you're trying to do]"),
        prompt_template: r#"The user is stuck$ARGUMENTS_SUFFIX. Help them get unstuck:

1. Clarify what they are trying to achieve (if unclear).
2. Identify why they might be stuck (missing context, unclear requirements, technical blocker).
3. Suggest 2–3 concrete next steps in order of likelihood of success.
4. If a technical blocker: propose specific debugging steps or workarounds.
5. Ask clarifying questions if needed.

Be direct and actionable. Focus on unblocking, not on explaining concepts."#,
        allowed_tools: None,
        user_invocable: true,
    },

    // -----------------------------------------------------------------------
    // batch
    // -----------------------------------------------------------------------
    BundledSkill {
        name: "batch",
        description: "Research and plan a large-scale change, then execute it in parallel across isolated worktree agents that each open a PR.",
        aliases: &[],
        when_to_use: Some("When the user wants to make a sweeping, mechanical change across many files that can be decomposed into independent parallel units."),
        argument_hint: Some("<instruction>"),
        prompt_template: r#"# Batch: Parallel Work Orchestration

You are orchestrating a large, parallelisable change across this codebase.

## User Instruction

$ARGUMENTS

## Phase 1: Research and Plan (Plan Mode)

Enter plan mode, then:

1. **Understand the scope.** Launch subagents to deeply research what this instruction
   touches. Find all files, patterns, and call sites that need to change.

2. **Decompose into independent units.** Break the work into 5–30 self-contained units.
   Each unit must be independently implementable in an isolated git worktree and
   mergeable on its own without depending on another unit's PR landing first.

3. **Determine the e2e test recipe.** Figure out how a worker can verify its change
   actually works end-to-end. If you cannot find a concrete path, ask the user.

4. **Write the plan.** Include: research summary, numbered work units, e2e recipe,
   and the exact worker instructions.

## Phase 2: Spawn Workers (After Plan Approval)

Spawn one background agent per work unit using the Agent tool with
`isolation: "worktree"` and `run_in_background: true`. Launch them all in a single
message block so they run in parallel. Each agent prompt must be fully self-contained.

After each agent finishes, parse the `PR: <url>` line from its result and render
a status table. When all agents have reported, print a final summary."#,
        allowed_tools: None,
        user_invocable: true,
    },

    // -----------------------------------------------------------------------
    // verify
    // -----------------------------------------------------------------------
    BundledSkill {
        name: "verify",
        description: "Verify that code or behaviour is correct.",
        aliases: &["check", "validate"],
        when_to_use: Some("After implementing something, to verify it is correct."),
        argument_hint: Some("[what to verify]"),
        prompt_template: r#"# Verify: $ARGUMENTS

## Verification Steps

1. Read the relevant code / implementation.
2. Check against requirements (if specified).
3. Look for edge cases and error conditions.
4. Run tests if available.
5. Check for common pitfalls: null handling, error propagation, type safety.
6. Report: what was verified, what passed, what failed or is uncertain."#,
        allowed_tools: None,
        user_invocable: true,
    },

    // -----------------------------------------------------------------------
    // update-config
    // -----------------------------------------------------------------------
    BundledSkill {
        name: "update-config",
        description: "Configure Claurst settings (hooks, permissions, env vars, behaviours) via settings.json.",
        aliases: &["config-update", "settings"],
        when_to_use: Some("When the user wants to configure automated behaviours, permissions, or settings."),
        argument_hint: Some("<what to configure>"),
        prompt_template: r#"# Update Config Skill

Modify Claurst configuration by updating settings.json files.

## Settings File Locations

| File | Scope | Use For |
|------|-------|---------|
| `~/.claude/settings.json` | Global | Personal preferences for all projects |
| `.claude/settings.json` | Project | Team-wide hooks, permissions, plugins |
| `.claude/settings.local.json` | Project (local) | Personal overrides for this project |

Settings load in order: user → project → local (later overrides earlier).

## CRITICAL: Read Before Write

Always read the existing settings file before making changes.
Merge new settings with existing ones — never replace the entire file.

## Hook Events

PreToolUse, PostToolUse, PreCompact, PostCompact, Stop, Notification, SessionStart

## User Request

$ARGUMENTS"#,
        allowed_tools: Some(&["Read", "Write", "Edit", "Bash"]),
        user_invocable: true,
    },

    // -----------------------------------------------------------------------
    // claude-api
    // -----------------------------------------------------------------------
    BundledSkill {
        name: "claude-api",
        description: "Build apps with the Claude API or Anthropic SDK.",
        aliases: &["api", "anthropic-sdk"],
        when_to_use: Some("When the user wants to use the Claude API, Anthropic SDK, or build Claude-powered apps."),
        argument_hint: Some("[what to build]"),
        prompt_template: r#"# Build a Claude API Integration

## User Request

$ARGUMENTS

## Default Models

- Most capable: claude-opus-4-6
- Balanced:     claude-sonnet-4-6
- Fast:         claude-haiku-4-5-20251001

## SDK Quickstart

**Python**
```python
pip install anthropic
import anthropic
client = anthropic.Anthropic()
```

**TypeScript / Node**
```typescript
npm install @anthropic-ai/sdk
import Anthropic from '@anthropic-ai/sdk';
const client = new Anthropic();
```

## Key API Features

- Streaming (`stream_message`)
- Tool use / function calling
- Extended thinking
- Prompt caching
- Vision (image input)
- Files API
- Batch processing

Use async/await patterns. Follow SDK best practices."#,
        allowed_tools: Some(&["Read", "Grep", "Glob", "WebFetch"]),
        user_invocable: true,
    },

    // -----------------------------------------------------------------------
    // loop
    // -----------------------------------------------------------------------
    BundledSkill {
        name: "loop",
        description: "Run a prompt or slash command on a recurring interval.",
        aliases: &[],
        when_to_use: Some("When the user wants to run something repeatedly on a schedule."),
        argument_hint: Some("[interval] <command>"),
        prompt_template: r#"# /loop — schedule a recurring prompt

Parse the input below into `[interval] <prompt…>` and schedule it with CronCreate.

## Parsing (in priority order)

1. **Leading token**: if the first token matches `^\d+[smhd]$` (e.g. `5m`, `2h`), that
   is the interval; the rest is the prompt.
2. **Trailing "every" clause**: if the input ends with `every <N><unit>` extract that
   as the interval and strip it from the prompt.
3. **Default**: interval is `10m` and the entire input is the prompt.

If the resulting prompt is empty, show usage `/loop [interval] <prompt>` and stop.

## Interval → Cron

| Pattern | Cron | Notes |
|---------|------|-------|
| `Nm` (N ≤ 59) | `*/N * * * *` | every N minutes |
| `Nh` (N ≤ 23) | `0 */N * * *` | every N hours |
| `Nd` | `0 0 */N * *` | every N days at midnight |
| `Ns` | round up to nearest minute | cron min granularity is 1 min |

## Action

1. Call CronCreate with the parsed cron expression and prompt.
2. Confirm what was scheduled, including the cron expression and human-readable cadence.
3. **Immediately execute the parsed prompt now** — don't wait for the first cron fire.

## Input

$ARGUMENTS"#,
        allowed_tools: Some(&["CronCreate", "CronList"]),
        user_invocable: true,
    },

    // -----------------------------------------------------------------------
    // arise
    // -----------------------------------------------------------------------
    BundledSkill {
        name: "arise",
        description: "Self-improving memory and knowledge system — combines a hierarchical wiki, MemPalace memory architecture, and session-based self-learning.",
        aliases: &["wiki", "memory-palace"],
        when_to_use: Some("When starting a new session (wake-up context), when the user wants to bootstrap project knowledge, or when ending a session to extract learnings."),
        argument_hint: Some("[bootstrap | sync | wake | learn | status]"),
        prompt_template: r#"# /arise — Self-Improving Memory & Knowledge System

You are the Arise system — a persistent, self-improving knowledge layer that combines
a hierarchical wiki (.claude/wiki/), a MemPalace memory architecture (.claude/palace/),
and session-based self-learning. Report in Spanish but keep technical terms in English.

## User Argument

The user invoked `/arise` with this argument (empty means no argument):

$ARGUMENTS

## Phase 0: Detect State

Read the user argument above. Then:
1. Check if `.claude/wiki/index.md` exists → set `WIKI_EXISTS`
2. Check if `.claude/palace/wings.json` exists → set `PALACE_EXISTS`
3. If argument is `wake` → skip to Phase 4
4. If argument is `learn` → skip to Phase 3
5. If argument is `status` → show wiki page count, palace wing/room/drawer counts, last sync date from wiki/log.md, then STOP
6. If argument is `sync` OR both WIKI_EXISTS and PALACE_EXISTS → go to Phase 2
7. If neither exists OR argument is `bootstrap` OR argument is empty → go to Phase 1

## Phase 1: Bootstrap (New Project)

Ask the user ONE question: **Personal or Laboral?**

### Personal Mode
Create these wiki files:
- `.claude/wiki/index.md` — master index linking all pages
- `.claude/wiki/personal/goals.md` — current goals
- `.claude/wiki/personal/projects.md` — active projects
- `.claude/wiki/personal/habits.md` — habits and routines
- `.claude/wiki/personal/tasks/week-current.md` — this week's tasks
- `.claude/wiki/personal/journal/` — directory for dated entries
- `.claude/wiki/log.md` — activity log (source of truth)

Initialize MemPalace:
- Wing: `personal` with rooms: `goals`, `projects`, `habits`, `tasks`, `journal`
- Each room starts empty — drawers are added as memories accumulate

### Laboral Mode
Ask follow-up: project name, team size, key goals (ONE prompt, all questions).
Create these wiki files:
- `.claude/wiki/index.md` — master index
- `.claude/wiki/laboral/org/structure.md` — org chart / team structure
- `.claude/wiki/laboral/team/members.md` — team members and roles
- `.claude/wiki/laboral/projects/active.md` — active projects
- `.claude/wiki/laboral/prd/template.md` — PRD template
- `.claude/wiki/laboral/transcripts/` — directory for meeting notes
- `.claude/wiki/laboral/decisions/log.md` — decision log (ADR-style)
- `.claude/wiki/log.md` — activity log

Initialize MemPalace:
- Wing: `laboral` with rooms: `org`, `team`, `projects`, `prd`, `transcripts`, `decisions`
- Wing: `technical` with rooms: `architecture`, `patterns`, `debt`, `learnings`

After bootstrap, log the event to `wiki/log.md` and confirm to user.

## Phase 2: Sync (Existing Project)

1. **Read all wiki files** — scan `.claude/wiki/` recursively
2. **Identify gaps** — pages with TODO markers, empty sections, stale dates
3. **Scan conversation history** — look for decisions, preferences, facts, milestones
4. **Extract memories** — for each extractable item:
   - Determine memory_type: `decision`, `preference`, `milestone`, `problem`, `pattern`, `fact`
   - File as a MemPalace drawer in the appropriate wing/room
   - If no matching room exists, create one
5. **Update wiki** — fill gaps with extracted information
6. **Update index** — ensure `.claude/wiki/index.md` links all pages
7. **Update knowledge graph** — add entity relationships discovered
8. **Log sync** to `wiki/log.md` with timestamp and summary

## Phase 3: Self-Learning Loop

Extract from the CURRENT session:
1. **Decisions made** — what was decided and why
2. **Preferences revealed** — coding style, tool choices, communication style
3. **Milestones reached** — features completed, bugs fixed, deploys done
4. **Problems encountered** — errors, blockers, workarounds
5. **Patterns discovered** — recurring code patterns, architectural choices

For each extracted item:
- Create a MemPalace drawer with tags: `memory_type`, `confidence`, `session_date`
- Add to knowledge graph if it involves entity relationships
- Cross-reference with existing drawers to avoid duplicates

Generate a **learnings summary** and append to `wiki/log.md`:
```
## Session: {date}
### Learnings
- {bullet list of what was learned}
### Memories Filed
- {count} new drawers across {wings touched}
### Knowledge Graph
- {new entities and relationships added}
```

## Phase 4: Wake-Up Context

Load essential context for session start (target: <900 tokens):
1. Read `wiki/log.md` — last 3 entries for recency
2. Load MemPalace L0 (identity layer) — who is the user, what is this project
3. Load MemPalace L1 (essential story) — key decisions, active goals, blockers
4. Compose a compact context block and present it:

```
=== ARISE WAKE-UP ===
Project: {name}
Mode: {personal|laboral}
Last session: {date} — {summary}
Active goals: {list}
Open blockers: {list}
Key decisions: {recent decisions}
=== END WAKE-UP ===
```

## Phase 5: Continuous Improvement (Background)

These rules apply ALWAYS, not just when /arise is invoked:
- On every `/compact`: extract memories from expiring context, update wiki
- On session end: run Phase 3 automatically
- Knowledge graph grows with each interaction
- Pattern detection improves entity recognition over time
- Never delete wiki pages — mark outdated content with ~~strikethrough~~
- Always update `wiki/log.md` as the source of truth
- Cross-link related wiki pages and MemPalace wings

## Tools to Use

- **Glob** — for checking file/directory existence and scanning wiki structure (prefer over Bash)
- **Read** — for loading existing wiki pages and palace state
- **Write / Edit** — for all wiki markdown files
- **MemPalace** — for ALL memory operations (init, add_drawer, search, kg_add, extract_memories, get_layers)
- **Bash** — ONLY for git operations (commit, log, status). Do NOT use Bash for file existence checks.
- **Agent** — for parallel extraction tasks when syncing large projects

## Autonomy

Minimize user interruptions. The only mandatory questions are:
1. Bootstrap: "Personal or Laboral?"
2. Laboral bootstrap: project name, team size, goals (one prompt)

Everything else runs autonomously. Report results, don't ask permission."#,
        allowed_tools: None,
        user_invocable: true,
    },

    // -----------------------------------------------------------------------
    // karpathy
    // -----------------------------------------------------------------------
    BundledSkill {
        name: "karpathy",
        description: "Autonomous experimentation loop — systematically optimizes any measurable metric through modify-measure-keep/discard cycles with agent teams.",
        aliases: &["autoresearch", "experiment"],
        when_to_use: Some("When the user wants to optimize a metric through systematic experimentation — performance, accuracy, cost, quality, or any measurable target."),
        argument_hint: Some("<what to optimize>"),
        prompt_template: r#"# /karpathy — Autonomous Experimentation Loop

You are running an autonomous Karpathy-style experimentation loop. The goal is to
systematically optimize a measurable metric through iterative experiments.

## User Goal

$ARGUMENTS

> If the User Goal section above is blank, STOP and ask: "What metric or system do you want to optimize?" Do NOT proceed without a clear goal.

## Phase 0: Domain Detection & Setup

Detect the domain from the user's goal:
- **ml** — model training, accuracy, loss metrics
- **web** — page speed, bundle size, Core Web Vitals, Lighthouse score
- **ads** — CTR, conversion rate, CPA, ROAS
- **code** — build time, test coverage, binary size, memory usage
- **custom** — user-defined metric with custom measurement

Based on domain, determine:
1. **Metric to optimize** — the single number to improve (e.g., Lighthouse score)
2. **Measurement command** — how to measure it (e.g., `lighthouse --output json`)
3. **Direction** — higher is better or lower is better
4. **Baseline** — measure the current value before any changes

If the domain or metric is ambiguous, ask the user ONE question to clarify.

## Phase 1: Initialize

1. Create experiment branch: `git checkout -b karpathy/{goal-slug}`
2. Measure baseline: run measurement command, record result
3. Initialize tracking files:
   - `results.tsv` — `experiment_id\tchange\tmetric_before\tmetric_after\tdelta\tkept`
   - `karpathy_learnings.md` — accumulated insights and patterns

Log baseline to `results.tsv`:
```
0\tbaseline\t-\t{baseline_value}\t-\tkept
```

## Phase 2: Experiment Loop

Repeat until the user stops or the goal is reached:

### Step 1: Hypothesize (Researcher Agent)
Launch an Agent to:
- Analyze current state and past experiment results
- Read `karpathy_learnings.md` for accumulated patterns
- Propose the SINGLE most promising change to try next
- Explain the hypothesis: "Changing X should improve metric because Y"
- Estimate expected improvement

### Step 2: Execute (Executor Agent)
Launch an Agent to:
- Implement the proposed change
- Keep changes minimal and reversible
- Commit with message: `karpathy: exp-{N} — {description}`

### Step 3: Measure & Decide (Analyst Agent)
Launch an Agent to:
- Run the measurement command
- Compare against previous best
- Decision:
  - **KEEP** if metric improved → record in results.tsv, update learnings
  - **DISCARD** if metric worsened or unchanged → `git revert HEAD --no-edit`, record in results.tsv

### Step 4: Log Results
Append to `results.tsv`:
```
{N}\t{change description}\t{before}\t{after}\t{delta}\t{kept|discarded}
```

Update `karpathy_learnings.md` with:
- What was tried
- What happened
- Why (hypothesis about the cause)
- What to try next based on this result

### Step 5: Report (Every 3 Experiments)
Every 3 experiments, output a progress report:
```
=== KARPATHY PROGRESS (Exp {N}) ===
Metric: {name}
Baseline: {value}
Current best: {value} ({improvement}%)
Experiments: {total} run, {kept} kept, {discarded} discarded
Last 3:
  - Exp {N-2}: {description} → {result}
  - Exp {N-1}: {description} → {result}
  - Exp {N}:   {description} → {result}
Top insight: {most useful learning so far}
=== END PROGRESS ===
```

## Phase 3: Stall Detection

Track consecutive discards. If **5 consecutive experiments are discarded**:

1. **Strategy Pivot** — the current approach is exhausted
2. Launch a Researcher Agent to:
   - Review ALL results.tsv entries
   - Identify what categories of changes have been tried
   - Propose a fundamentally different strategy
   - Update `karpathy_learnings.md` with pivot reasoning
3. Reset consecutive discard counter
4. Continue with new strategy

## Phase 4: MemPalace Integration (Optional)

First check if `.claude/palace/wings.json` exists. If it does NOT exist, skip this phase
and note in the progress report: "Run `/arise bootstrap` to enable MemPalace integration."

If the palace exists, after each experiment:
- Use the **MemPalace** tool with action `add_drawer`, wing `experiments`, room `{domain}`
  - Tags: `experiment`, `{kept|discarded}`, `{domain}`, `session_date`
- If a pattern emerges (3+ similar results), add to knowledge graph:
  - Entity: the technique/approach
  - Relationship: `improves`/`degrades` the metric
  - Confidence: based on consistency of results

After every progress report:
- Extract meta-patterns and file as `pattern` type drawers
- Update knowledge graph with technique relationships

## Phase 5: Completion

When the user says stop, or the target is reached:
1. Output final summary with all results
2. Commit final state: `karpathy: final — {metric} improved from {baseline} to {best}`
3. File comprehensive summary as MemPalace drawer
4. Update `karpathy_learnings.md` with final conclusions

## Agent Team Roles

- **Researcher**: Analyzes data, proposes hypotheses, identifies patterns
- **Executor**: Implements changes, keeps them minimal and clean
- **Analyst**: Measures results, makes keep/discard decisions objectively

Launch agents in parallel when possible (e.g., Researcher can start while Analyst logs).

## Rules

1. ONE change per experiment — never bundle multiple changes
2. Always measure before and after — no guessing
3. Revert discarded experiments completely — keep the branch clean
4. Log EVERYTHING to results.tsv — it's the source of truth
5. Be autonomous — only ask the user if measurement setup is unclear
6. Report in Spanish but keep metric names and technical terms in English
7. Never skip measurement — if the measurement command fails, fix it first"#,
        allowed_tools: None,
        user_invocable: true,
    },
];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Find a bundled skill by name or alias (case-insensitive).
pub fn find_bundled_skill(name: &str) -> Option<&'static BundledSkill> {
    let lower = name.to_lowercase();
    BUNDLED_SKILLS.iter().find(|s| {
        s.name == lower || s.aliases.iter().any(|a| *a == lower)
    })
}

/// Return `(name, description)` pairs for all user-invocable bundled skills.
pub fn user_invocable_skills() -> Vec<(&'static str, &'static str)> {
    BUNDLED_SKILLS
        .iter()
        .filter(|s| s.user_invocable)
        .map(|s| (s.name, s.description))
        .collect()
}

/// Expand a skill's prompt template, substituting `$ARGUMENTS` and
/// `$ARGUMENTS_SUFFIX`.
///
/// - `$ARGUMENTS`        → replaced by `args` verbatim (or `""` when empty)
/// - `$ARGUMENTS_SUFFIX` → replaced by `": <args>"` when non-empty, else `""`
pub fn expand_prompt(skill: &BundledSkill, args: &str) -> String {
    let suffix = if args.is_empty() {
        String::new()
    } else {
        format!(": {}", args)
    };

    skill
        .prompt_template
        .replace("$ARGUMENTS_SUFFIX", &suffix)
        .replace("$ARGUMENTS", args)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_skills_have_non_empty_names() {
        for s in BUNDLED_SKILLS {
            assert!(!s.name.is_empty(), "skill has empty name");
        }
    }

    #[test]
    fn all_skills_have_non_empty_descriptions() {
        for s in BUNDLED_SKILLS {
            assert!(
                !s.description.is_empty(),
                "skill '{}' has empty description",
                s.name
            );
        }
    }

    #[test]
    fn all_skills_have_non_empty_prompt_templates() {
        for s in BUNDLED_SKILLS {
            assert!(
                !s.prompt_template.is_empty(),
                "skill '{}' has empty prompt_template",
                s.name
            );
        }
    }

    #[test]
    fn skill_names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for s in BUNDLED_SKILLS {
            assert!(
                seen.insert(s.name),
                "duplicate skill name: {}",
                s.name
            );
        }
    }

    #[test]
    fn find_by_primary_name() {
        let skill = find_bundled_skill("simplify");
        assert!(skill.is_some());
        assert_eq!(skill.unwrap().name, "simplify");
    }

    #[test]
    fn find_by_alias() {
        let skill = find_bundled_skill("mem");
        assert!(skill.is_some());
        assert_eq!(skill.unwrap().name, "remember");
    }

    #[test]
    fn find_case_insensitive() {
        assert!(find_bundled_skill("SIMPLIFY").is_some());
        assert!(find_bundled_skill("Debug").is_some());
    }

    #[test]
    fn find_missing_returns_none() {
        assert!(find_bundled_skill("nonexistent-skill-xyz").is_none());
    }

    #[test]
    fn expand_prompt_substitutes_arguments() {
        let skill = find_bundled_skill("debug").unwrap();
        let expanded = expand_prompt(skill, "NullPointerException in Foo.java");
        assert!(expanded.contains("NullPointerException in Foo.java"));
        assert!(!expanded.contains("$ARGUMENTS"));
    }

    #[test]
    fn expand_prompt_empty_args_no_residual_placeholder() {
        let skill = find_bundled_skill("simplify").unwrap();
        let expanded = expand_prompt(skill, "");
        assert!(!expanded.contains("$ARGUMENTS"));
        assert!(!expanded.contains("$ARGUMENTS_SUFFIX"));
    }

    #[test]
    fn expand_prompt_suffix_non_empty() {
        let skill = find_bundled_skill("stuck").unwrap();
        let expanded = expand_prompt(skill, "trying to run tests");
        // Should contain ": trying to run tests" from $ARGUMENTS_SUFFIX
        assert!(expanded.contains(": trying to run tests"));
    }

    #[test]
    fn expand_prompt_suffix_empty() {
        let skill = find_bundled_skill("stuck").unwrap();
        let expanded = expand_prompt(skill, "");
        // $ARGUMENTS_SUFFIX should expand to "" so "stuck" is not followed by ": "
        assert!(!expanded.contains("stuck: "));
        assert!(!expanded.contains("$ARGUMENTS_SUFFIX"));
    }

    #[test]
    fn user_invocable_skills_non_empty() {
        let skills = user_invocable_skills();
        assert!(!skills.is_empty());
    }

    #[test]
    fn user_invocable_skills_all_marked_true() {
        for (name, _) in user_invocable_skills() {
            let skill = find_bundled_skill(name).unwrap();
            assert!(
                skill.user_invocable,
                "skill '{}' returned by user_invocable_skills() but user_invocable=false",
                name
            );
        }
    }
}
