# MemPalace — Self-Learning Memory System

## Overview

MemPalace is a local-first memory system built into Claurst that stores everything verbatim and makes it searchable. Combined with the `/arise` skill, it creates a self-improving knowledge system that compounds across sessions.

## Architecture

### The Palace Metaphor

```
WING (project/person/topic)
  └─ ROOM (specific area)
      └─ DRAWER (verbatim content chunk)
```

**Wings** are top-level buckets (e.g., `my_project`, `team`, `architecture`).
**Rooms** are topics within a wing (e.g., `frontend`, `backend`, `decisions`).
**Drawers** are individual content chunks stored with full metadata.

### Knowledge Graph

Temporal entity relationships stored as subject → predicate → object triples:

```
("Max", "works_on", "frontend") valid_from: 2026-01-01
("API", "uses", "GraphQL")      valid_from: 2026-03-15
```

Supports time-aware queries: "What was true about X in January?"

### 4-Layer Memory Stack

| Layer | Content | Tokens | When Loaded |
|-------|---------|--------|-------------|
| L0 | Identity (who am I, what do I work on) | ~100 | Always |
| L1 | Essential story (recent important memories) | ~500 | Always at wake-up |
| L2 | Wing-specific memories | ~200-500 | When wing is active |
| L3 | Full search results | Unlimited | On-demand queries |

Wake-up cost: L0 + L1 ≈ 600 tokens. Leaves 95%+ of context free.

## MemPalace Tool

### Actions

| Action | Description | Key Params |
|--------|-------------|------------|
| `init` | Initialize palace structure | `wing` (optional) |
| `add_drawer` | Store verbatim content | `wing`, `room`, `content`, `memory_type` |
| `search` | Keyword search across drawers | `query`, `wing`, `room`, `max_results` |
| `list_wings` | List all wings with counts | — |
| `list_rooms` | List rooms in a wing | `wing` |
| `status` | Palace stats overview | — |
| `kg_add` | Add knowledge graph triple | `subject`, `predicate`, `object`, `valid_from` |
| `kg_query` | Query entity relationships | `entity`, `as_of`, `direction` |
| `kg_invalidate` | Mark triple as no longer valid | `subject`, `predicate`, `object` |
| `extract_memories` | Pattern-extract memory types from text | `content`, `wing`, `room` |
| `get_layers` | Get layered memory context | `layer` (0-3), `wing`, `query` |
| `delete_drawer` | Remove drawer by ID | `drawer_id` |

### Memory Types (auto-extracted)

| Type | Markers |
|------|---------|
| Decision | "decided", "let's use", "switched to", "going with" |
| Preference | "always use", "never do", "prefer", "I like" |
| Milestone | "got it working", "finally", "shipped", "launched" |
| Problem | "bug", "error", "crash", "root cause" |
| Emotional | "love", "hate", "proud", "frustrated", "excited" |

## /arise Skill

Invoke with `/arise` to bootstrap or sync the self-learning system.

### Modes

**Personal** — Tasks, goals, habits, journal, projects
**Laboral** — Org chart, team roster, PRDs, transcripts, decisions

### What It Creates

```
.claude/
├── wiki/
│   ├── index.md
│   ├── log.md
│   ├── overview.md
│   └── personal/ or laboral/
└── palace/
    ├── wings.json
    ├── knowledge_graph.json
    ├── identity.txt
    ├── learnings.json
    └── wings/
        └── {wing_name}/{room_name}/*.json
```

### Self-Learning Cycle

1. **Extract** — Finds decisions, preferences, milestones, problems in conversation
2. **File** — Stores as MemPalace drawers with proper tags
3. **Connect** — Updates knowledge graph with entity relationships
4. **Learn** — Generates learnings summary
5. **Compound** — Each session builds on previous knowledge

## /karpathy Skill

Autonomous experimentation loop: modify → measure → keep/discard → repeat.

### Domains

- **ml** — Training runs (val_bpb optimization)
- **web** — Lighthouse scores, Core Web Vitals
- **ads** — CTR, ROAS, CPC optimization
- **code** — Test coverage, build time, bundle size
- **custom** — Any measurable metric

### Agent Team

| Role | Agent | Responsibility |
|------|-------|---------------|
| Lead | You | Orchestrates loop |
| Researcher | karpathy-researcher | Proposes experiments |
| Executor | karpathy-executor | Implements in worktrees |
| Analyst | karpathy-analyst | Evaluates keep/discard |

### MemPalace Integration

- Experiment results filed as drawers in `experiments` wing
- Patterns tracked in knowledge graph
- Learnings compound across experiment runs

## Storage

All data is pure JSON on the filesystem — no external databases, no API calls. Everything runs locally and works offline.

```
.claude/palace/
├── wings.json              # {"wings": [{"name": "...", "created_at": "..."}]}
├── knowledge_graph.json    # {"triples": [{subject, predicate, object, ...}]}
├── identity.txt            # Free-text identity description
├── learnings.json          # {"entries": [{pattern, category, discovered_at}]}
└── wings/
    └── my_project/
        ├── frontend/
        │   ├── drawer_my_project_frontend_a1b2c3d4.json
        │   └── drawer_my_project_frontend_e5f6g7h8.json
        └── decisions/
            └── drawer_my_project_decisions_i9j0k1l2.json
```
