// MemPalace tool: local-first memory system using the "memory palace" metaphor.
//
// Stores verbatim content organized hierarchically:
//   WING (project/person/topic)
//     └─ ROOM (specific area within wing)
//         └─ DRAWER (individual content chunk)
//
// Plus a Knowledge Graph for temporal entity relationships.

use crate::{PermissionLevel, Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tracing::debug;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const PALACE_DIR: &str = ".claude/palace";
const WINGS_INDEX: &str = "wings.json";
const KG_FILE: &str = "knowledge_graph.json";
const IDENTITY_FILE: &str = "identity.txt";
const LEARNINGS_FILE: &str = "learnings.json";
const DEFAULT_MAX_RESULTS: usize = 5;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Drawer {
    id: String,
    content: String,
    wing: String,
    room: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default = "default_memory_type")]
    memory_type: String,
    filed_at: String,
    added_by: String,
}

fn default_memory_type() -> String {
    "general".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WingsIndex {
    wings: Vec<WingEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WingEntry {
    name: String,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KnowledgeGraph {
    triples: Vec<KgTriple>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KgTriple {
    subject: String,
    predicate: String,
    object: String,
    #[serde(default)]
    valid_from: Option<String>,
    #[serde(default)]
    valid_to: Option<String>,
    #[serde(default = "default_confidence")]
    confidence: f64,
    created_at: String,
}

fn default_confidence() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SearchResult {
    drawer: Drawer,
    score: f64,
}

// ---------------------------------------------------------------------------
// Hash helper
// ---------------------------------------------------------------------------

fn simple_hash(input: &str) -> String {
    let mut hash: u64 = 0;
    for byte in input.as_bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(*byte as u64);
    }
    let hex = format!("{:016x}", hash);
    hex[..8].to_string()
}

// ---------------------------------------------------------------------------
// Palace path helpers
// ---------------------------------------------------------------------------

fn palace_root(ctx: &ToolContext) -> PathBuf {
    ctx.working_dir.join(PALACE_DIR)
}

fn wings_index_path(ctx: &ToolContext) -> PathBuf {
    palace_root(ctx).join(WINGS_INDEX)
}

fn wing_dir(ctx: &ToolContext, wing: &str) -> PathBuf {
    palace_root(ctx).join("wings").join(sanitize_name(wing))
}

fn room_dir(ctx: &ToolContext, wing: &str, room: &str) -> PathBuf {
    wing_dir(ctx, wing).join(sanitize_name(room))
}

fn kg_path(ctx: &ToolContext) -> PathBuf {
    palace_root(ctx).join(KG_FILE)
}

fn identity_path(ctx: &ToolContext) -> PathBuf {
    palace_root(ctx).join(IDENTITY_FILE)
}

fn learnings_path(ctx: &ToolContext) -> PathBuf {
    palace_root(ctx).join(LEARNINGS_FILE)
}

/// Sanitize a name for use as a directory/file name.
/// Returns None if the name is empty or contains only non-alphanumeric characters.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>()
        .to_lowercase()
}



fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

// ---------------------------------------------------------------------------
// File I/O helpers
// ---------------------------------------------------------------------------

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let data = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&data) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "Corrupted JSON in palace file");
            None
        }
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
    }
    let serialized = serde_json::to_string_pretty(value)
        .map_err(|e| format!("Serialization error: {}", e))?;
    // Atomic write: write to .tmp then rename to prevent corruption on crash/race
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, &serialized)
        .map_err(|e| format!("Failed to write temp file {}: {}", tmp_path.display(), e))?;
    std::fs::rename(&tmp_path, path)
        .map_err(|e| {
            // Fallback: if rename fails (cross-device), write directly
            let _ = std::fs::remove_file(&tmp_path);
            std::fs::write(path, &serialized)
                .map_err(|e2| format!("Failed to write {}: {}", path.display(), e2))
                .err()
                .unwrap_or_default();
            format!("Rename failed ({}), wrote directly", e)
        })
        .or(Ok(()))
}

fn load_wings_index(ctx: &ToolContext) -> WingsIndex {
    read_json(&wings_index_path(ctx)).unwrap_or(WingsIndex { wings: vec![] })
}

fn save_wings_index(ctx: &ToolContext, index: &WingsIndex) -> Result<(), String> {
    write_json(&wings_index_path(ctx), index)
}

fn load_kg(ctx: &ToolContext) -> KnowledgeGraph {
    read_json(&kg_path(ctx)).unwrap_or(KnowledgeGraph { triples: vec![] })
}

fn save_kg(ctx: &ToolContext, kg: &KnowledgeGraph) -> Result<(), String> {
    write_json(&kg_path(ctx), kg)
}

/// Collect all drawers across all wings and rooms.
fn all_drawers(ctx: &ToolContext) -> Vec<Drawer> {
    let mut drawers = Vec::new();
    let wings_dir = palace_root(ctx).join("wings");
    if !wings_dir.exists() {
        return drawers;
    }
    let wing_entries = match std::fs::read_dir(&wings_dir) {
        Ok(entries) => entries,
        Err(_) => return drawers,
    };
    for wing_entry in wing_entries.flatten() {
        if !wing_entry.path().is_dir() {
            continue;
        }
        let room_entries = match std::fs::read_dir(wing_entry.path()) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for room_entry in room_entries.flatten() {
            if !room_entry.path().is_dir() {
                continue;
            }
            let file_entries = match std::fs::read_dir(room_entry.path()) {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            for file_entry in file_entries.flatten() {
                let path = file_entry.path();
                if path.extension().map_or(true, |ext| ext != "json") {
                    continue;
                }
                if let Some(drawer) = read_json::<Drawer>(&path) {
                    drawers.push(drawer);
                }
            }
        }
    }
    drawers
}

/// Collect drawers from a specific wing.
fn drawers_in_wing(ctx: &ToolContext, wing: &str) -> Vec<Drawer> {
    let mut drawers = Vec::new();
    let wdir = wing_dir(ctx, wing);
    if !wdir.exists() {
        return drawers;
    }
    let room_entries = match std::fs::read_dir(&wdir) {
        Ok(entries) => entries,
        Err(_) => return drawers,
    };
    for room_entry in room_entries.flatten() {
        if !room_entry.path().is_dir() {
            continue;
        }
        let file_entries = match std::fs::read_dir(room_entry.path()) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for file_entry in file_entries.flatten() {
            let path = file_entry.path();
            if path.extension().map_or(true, |ext| ext != "json") {
                continue;
            }
            if let Some(drawer) = read_json::<Drawer>(&path) {
                drawers.push(drawer);
            }
        }
    }
    drawers
}

/// Collect drawers from a specific wing + room.
fn drawers_in_room(ctx: &ToolContext, wing: &str, room: &str) -> Vec<Drawer> {
    let mut drawers = Vec::new();
    let rdir = room_dir(ctx, wing, room);
    if !rdir.exists() {
        return drawers;
    }
    let file_entries = match std::fs::read_dir(&rdir) {
        Ok(entries) => entries,
        Err(_) => return drawers,
    };
    for file_entry in file_entries.flatten() {
        let path = file_entry.path();
        if path.extension().map_or(true, |ext| ext != "json") {
            continue;
        }
        if let Some(drawer) = read_json::<Drawer>(&path) {
            drawers.push(drawer);
        }
    }
    drawers
}

// ---------------------------------------------------------------------------
// Search scoring (TF-IDF style keyword matching)
// ---------------------------------------------------------------------------

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty() && s.len() > 1)
        .map(|s| s.to_string())
        .collect()
}

fn keyword_score(query_tokens: &[String], content: &str) -> f64 {
    let content_tokens = tokenize(content);
    if content_tokens.is_empty() {
        return 0.0;
    }
    // Count how many query tokens appear in content
    let mut matches = 0usize;
    for qt in query_tokens {
        for ct in &content_tokens {
            if ct == qt {
                matches += 1;
            }
        }
    }
    // Normalize: matches / sqrt(query_len * content_len) for TF-IDF-like behavior
    let query_len = query_tokens.len() as f64;
    let content_len = content_tokens.len() as f64;
    if query_len == 0.0 {
        return 0.0;
    }
    (matches as f64) / (query_len * content_len).sqrt()
}

// ---------------------------------------------------------------------------
// Memory extraction patterns
// ---------------------------------------------------------------------------

struct MemoryPattern {
    memory_type: &'static str,
    keywords: &'static [&'static str],
}

const MEMORY_PATTERNS: &[MemoryPattern] = &[
    MemoryPattern {
        memory_type: "decision",
        keywords: &["decided", "let's use", "switched to", "going with", "chose"],
    },
    MemoryPattern {
        memory_type: "preference",
        keywords: &["always use", "never do", "prefer", "i like", "don't like"],
    },
    MemoryPattern {
        memory_type: "milestone",
        keywords: &["got it working", "finally", "breakthrough", "shipped", "completed", "launched"],
    },
    MemoryPattern {
        memory_type: "problem",
        keywords: &["bug", "error", "crash", "broken", "root cause", "issue"],
    },
    MemoryPattern {
        memory_type: "emotional",
        keywords: &["love", "hate", "proud", "frustrated", "excited", "scared"],
    },
];

/// Extract sentences matching memory patterns from text content.
fn extract_memory_segments(content: &str) -> Vec<(String, String)> {
    let sentences: Vec<&str> = content
        .split(|c: char| c == '.' || c == '!' || c == '?' || c == '\n')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let mut results: Vec<(String, String)> = Vec::new();

    for sentence in &sentences {
        let sentence_lower = sentence.to_lowercase();
        for pattern in MEMORY_PATTERNS {
            let matched = pattern.keywords.iter().any(|kw| sentence_lower.contains(kw));
            if matched {
                results.push((sentence.to_string(), pattern.memory_type.to_string()));
                break; // one type per sentence
            }
        }
    }

    results
}

// ---------------------------------------------------------------------------
// Action implementations
// ---------------------------------------------------------------------------

fn action_init(input: &Value, ctx: &ToolContext) -> ToolResult {
    let root = palace_root(ctx);

    // Create base structure
    let dirs_to_create = [
        root.clone(),
        root.join("wings"),
    ];
    for dir in &dirs_to_create {
        if let Err(e) = std::fs::create_dir_all(dir) {
            return ToolResult::error(format!("Failed to create directory {}: {}", dir.display(), e));
        }
    }

    // Initialize wings index if missing
    if !wings_index_path(ctx).exists() {
        if let Err(e) = save_wings_index(ctx, &WingsIndex { wings: vec![] }) {
            return ToolResult::error(format!("Failed to create wings index: {}", e));
        }
    }

    // Initialize KG if missing
    if !kg_path(ctx).exists() {
        if let Err(e) = save_kg(ctx, &KnowledgeGraph { triples: vec![] }) {
            return ToolResult::error(format!("Failed to create knowledge graph: {}", e));
        }
    }

    // Initialize identity file if missing
    if !identity_path(ctx).exists() {
        if let Err(e) = std::fs::write(
            identity_path(ctx),
            "# Identity\nThis palace belongs to the project owner.\n",
        ) {
            return ToolResult::error(format!("Failed to create identity file: {}", e));
        }
    }

    // Initialize learnings file if missing
    if !learnings_path(ctx).exists() {
        if let Err(e) = write_json(&learnings_path(ctx), &json!([])) {
            return ToolResult::error(format!("Failed to create learnings file: {}", e));
        }
    }

    // Optionally create a specific wing
    let mut msg = format!("Palace initialized at {}", root.display());
    if let Some(wing_name) = input.get("wing").and_then(|v| v.as_str()) {
        let wdir = wing_dir(ctx, wing_name);
        if let Err(e) = std::fs::create_dir_all(&wdir) {
            return ToolResult::error(format!("Failed to create wing directory: {}", e));
        }
        // Register wing in index
        let mut index = load_wings_index(ctx);
        let sanitized = sanitize_name(wing_name);
        if !index.wings.iter().any(|w| w.name == sanitized) {
            index.wings.push(WingEntry {
                name: sanitized.clone(),
                created_at: now_iso(),
            });
            if let Err(e) = save_wings_index(ctx, &index) {
                return ToolResult::error(format!("Failed to update wings index: {}", e));
            }
        }
        msg = format!("{}\nWing '{}' created.", msg, sanitized);
    }

    ToolResult::success(msg)
}

fn action_add_drawer(input: &Value, ctx: &ToolContext) -> ToolResult {
    let wing = match input.get("wing").and_then(|v| v.as_str()) {
        Some(w) => w,
        None => return ToolResult::error("Missing required parameter: wing"),
    };
    let room = match input.get("room").and_then(|v| v.as_str()) {
        Some(r) => r,
        None => return ToolResult::error("Missing required parameter: room"),
    };
    let content = match input.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return ToolResult::error("Missing required parameter: content"),
    };
    let source = input.get("source").and_then(|v| v.as_str()).map(|s| s.to_string());
    let memory_type = input
        .get("memory_type")
        .and_then(|v| v.as_str())
        .unwrap_or("general")
        .to_string();

    // Validate memory_type
    let valid_types = ["decision", "preference", "milestone", "problem", "emotional", "general"];
    if !valid_types.contains(&memory_type.as_str()) {
        return ToolResult::error(format!(
            "Invalid memory_type '{}'. Must be one of: {}",
            memory_type,
            valid_types.join(", ")
        ));
    }

    // Content size cap: 64 KB
    if content.len() > 65_536 {
        return ToolResult::error("Content exceeds 64 KB limit. Split into smaller chunks.");
    }

    let sanitized_wing = sanitize_name(wing);
    let sanitized_room = sanitize_name(room);

    if sanitized_wing.is_empty() {
        return ToolResult::error("Wing name must contain at least one alphanumeric character.");
    }
    if sanitized_room.is_empty() {
        return ToolResult::error("Room name must contain at least one alphanumeric character.");
    }

    // Ensure wing exists in index
    let mut index = load_wings_index(ctx);
    if !index.wings.iter().any(|w| w.name == sanitized_wing) {
        index.wings.push(WingEntry {
            name: sanitized_wing.clone(),
            created_at: now_iso(),
        });
        if let Err(e) = save_wings_index(ctx, &index) {
            return ToolResult::error(format!("Failed to update wings index: {}", e));
        }
    }

    // Ensure room directory exists
    let rdir = room_dir(ctx, wing, room);
    if let Err(e) = std::fs::create_dir_all(&rdir) {
        return ToolResult::error(format!("Failed to create room directory: {}", e));
    }

    // Generate drawer ID — include timestamp to prevent collision-based overwrites
    let ts = Utc::now().timestamp_millis();
    let hash_input = format!("{}{}{}{}", wing, room, content, ts);
    let drawer_id = format!(
        "drawer_{}_{}_{}_{}", sanitized_wing, sanitized_room, simple_hash(&hash_input), ts
    );

    let drawer = Drawer {
        id: drawer_id.clone(),
        content: content.to_string(),
        wing: sanitized_wing.clone(),
        room: sanitized_room.clone(),
        source,
        memory_type,
        filed_at: now_iso(),
        added_by: "claurst".to_string(),
    };

    let drawer_path = rdir.join(format!("{}.json", drawer_id));
    if let Err(e) = write_json(&drawer_path, &drawer) {
        return ToolResult::error(format!("Failed to write drawer: {}", e));
    }

    debug!(drawer_id = %drawer.id, wing = %drawer.wing, room = %drawer.room, "Added drawer");

    ToolResult::success(format!(
        "Drawer '{}' filed in {}/{} (type: {})",
        drawer_id, sanitized_wing, sanitized_room, drawer.memory_type
    ))
}

fn action_search(input: &Value, ctx: &ToolContext) -> ToolResult {
    let query = match input.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return ToolResult::error("Missing required parameter: query"),
    };
    let wing_filter = input.get("wing").and_then(|v| v.as_str());
    let room_filter = input.get("room").and_then(|v| v.as_str());
    let max_results = input
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_MAX_RESULTS as u64)
        .min(100) as usize;

    // Collect candidate drawers
    let candidates = match (wing_filter, room_filter) {
        (Some(w), Some(r)) => drawers_in_room(ctx, w, r),
        (Some(w), None) => drawers_in_wing(ctx, w),
        _ => all_drawers(ctx),
    };

    if candidates.is_empty() {
        return ToolResult::success("No drawers found matching the search criteria.");
    }

    let query_tokens = tokenize(query);

    // Score and rank
    let mut scored: Vec<SearchResult> = candidates
        .into_iter()
        .map(|d| {
            // Score content + source + memory_type
            let mut text = d.content.clone();
            if let Some(ref src) = d.source {
                text.push(' ');
                text.push_str(src);
            }
            text.push(' ');
            text.push_str(&d.memory_type);
            let score = keyword_score(&query_tokens, &text);
            SearchResult { drawer: d, score }
        })
        .filter(|sr| sr.score > 0.0)
        .collect();

    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(max_results);

    if scored.is_empty() {
        return ToolResult::success("No matching drawers found for the given query.");
    }

    let results: Vec<Value> = scored
        .iter()
        .map(|sr| {
            json!({
                "id": sr.drawer.id,
                "wing": sr.drawer.wing,
                "room": sr.drawer.room,
                "content": sr.drawer.content,
                "memory_type": sr.drawer.memory_type,
                "source": sr.drawer.source,
                "filed_at": sr.drawer.filed_at,
                "score": format!("{:.3}", sr.score),
            })
        })
        .collect();

    let output = serde_json::to_string_pretty(&results).unwrap_or_else(|e| format!("Serialization error: {}", e));
    ToolResult::success(format!("Found {} result(s):\n{}", results.len(), output))
}

fn action_list_wings(ctx: &ToolContext) -> ToolResult {
    let index = load_wings_index(ctx);
    if index.wings.is_empty() {
        return ToolResult::success("No wings found. Use action 'init' with a 'wing' param to create one.");
    }

    let mut output = format!("Wings ({}):\n", index.wings.len());
    for wing in &index.wings {
        let count = drawers_in_wing(ctx, &wing.name).len();
        output.push_str(&format!("  - {} ({} drawers, created: {})\n", wing.name, count, wing.created_at));
    }

    ToolResult::success(output)
}

fn action_list_rooms(input: &Value, ctx: &ToolContext) -> ToolResult {
    let wing = match input.get("wing").and_then(|v| v.as_str()) {
        Some(w) => w,
        None => return ToolResult::error("Missing required parameter: wing"),
    };

    let wdir = wing_dir(ctx, wing);
    if !wdir.exists() {
        return ToolResult::error(format!("Wing '{}' does not exist.", sanitize_name(wing)));
    }

    let room_entries = match std::fs::read_dir(&wdir) {
        Ok(entries) => entries,
        Err(e) => return ToolResult::error(format!("Failed to read wing directory: {}", e)),
    };

    let mut rooms: Vec<(String, usize)> = Vec::new();
    for entry in room_entries.flatten() {
        if entry.path().is_dir() {
            let room_name = entry
                .file_name()
                .to_string_lossy()
                .to_string();
            let count = drawers_in_room(ctx, wing, &room_name).len();
            rooms.push((room_name, count));
        }
    }

    if rooms.is_empty() {
        return ToolResult::success(format!("Wing '{}' has no rooms yet.", sanitize_name(wing)));
    }

    let mut output = format!("Rooms in '{}' ({}):\n", sanitize_name(wing), rooms.len());
    for (name, count) in &rooms {
        output.push_str(&format!("  - {} ({} drawers)\n", name, count));
    }

    ToolResult::success(output)
}

fn action_status(ctx: &ToolContext) -> ToolResult {
    let index = load_wings_index(ctx);
    let all = all_drawers(ctx);
    let kg = load_kg(ctx);

    // Count rooms
    let mut room_count = 0usize;
    for wing in &index.wings {
        let wdir = wing_dir(ctx, &wing.name);
        if let Ok(entries) = std::fs::read_dir(&wdir) {
            room_count += entries.flatten().filter(|e| e.path().is_dir()).count();
        }
    }

    // Find last activity
    let last_activity = all
        .iter()
        .map(|d| d.filed_at.as_str())
        .max()
        .unwrap_or("never");

    let has_identity = identity_path(ctx).exists();
    let kg_count = kg.triples.len();

    let output = format!(
        "MemPalace Status:\n\
         - Wings:         {}\n\
         - Rooms:         {}\n\
         - Total drawers: {}\n\
         - KG triples:    {}\n\
         - Identity:      {}\n\
         - Last activity: {}\n\
         - Palace root:   {}",
        index.wings.len(),
        room_count,
        all.len(),
        kg_count,
        if has_identity { "present" } else { "missing" },
        last_activity,
        palace_root(ctx).display(),
    );

    ToolResult::success(output)
}

fn action_kg_add(input: &Value, ctx: &ToolContext) -> ToolResult {
    let subject = match input.get("subject").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return ToolResult::error("Missing required parameter: subject"),
    };
    let predicate = match input.get("predicate").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return ToolResult::error("Missing required parameter: predicate"),
    };
    let object = match input.get("object").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return ToolResult::error("Missing required parameter: object"),
    };
    let valid_from = input.get("valid_from").and_then(|v| v.as_str()).map(|s| s.to_string());
    let valid_to = input.get("valid_to").and_then(|v| v.as_str()).map(|s| s.to_string());
    let confidence = input.get("confidence").and_then(|v| v.as_f64()).unwrap_or(1.0);

    // Validate confidence range
    if !(0.0..=1.0).contains(&confidence) {
        return ToolResult::error("Confidence must be between 0.0 and 1.0.");
    }

    let mut kg = load_kg(ctx);

    // Cap KG size to prevent unbounded growth
    if kg.triples.len() >= 10_000 {
        return ToolResult::error(
            "Knowledge graph has reached the 10,000 triple limit. \
             Invalidate or remove old triples before adding new ones."
        );
    }

    let triple = KgTriple {
        subject: subject.clone(),
        predicate: predicate.clone(),
        object: object.clone(),
        valid_from,
        valid_to,
        confidence,
        created_at: now_iso(),
    };

    kg.triples.push(triple);
    if let Err(e) = save_kg(ctx, &kg) {
        return ToolResult::error(format!("Failed to save knowledge graph: {}", e));
    }

    debug!(%subject, %predicate, %object, "Added KG triple");
    ToolResult::success(format!(
        "KG triple added: {} --[{}]--> {} (confidence: {:.2})",
        subject, predicate, object, confidence
    ))
}

fn action_kg_query(input: &Value, ctx: &ToolContext) -> ToolResult {
    let entity = match input.get("entity").and_then(|v| v.as_str()) {
        Some(e) => e,
        None => return ToolResult::error("Missing required parameter: entity"),
    };
    let as_of = input.get("as_of").and_then(|v| v.as_str());
    let direction = input
        .get("direction")
        .and_then(|v| v.as_str())
        .unwrap_or("both");

    let kg = load_kg(ctx);
    let entity_lower = entity.to_lowercase();

    let mut results: Vec<&KgTriple> = Vec::new();
    for triple in &kg.triples {
        let is_subject = triple.subject.to_lowercase() == entity_lower;
        let is_object = triple.object.to_lowercase() == entity_lower;

        let direction_match = match direction {
            "outgoing" => is_subject,
            "incoming" => is_object,
            _ => is_subject || is_object,
        };

        if !direction_match {
            continue;
        }

        // Check temporal validity if as_of is provided (parse dates with chrono)
        if let Some(date_str) = as_of {
            if let Ok(query_date) = chrono::DateTime::parse_from_rfc3339(date_str)
                .map(|d| d.with_timezone(&Utc))
                .or_else(|_| {
                    chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                        .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
                })
            {
                if let Some(ref vf) = triple.valid_from {
                    if let Ok(from) = chrono::DateTime::parse_from_rfc3339(vf)
                        .map(|d| d.with_timezone(&Utc))
                        .or_else(|_| {
                            chrono::NaiveDate::parse_from_str(vf, "%Y-%m-%d")
                                .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
                        })
                    {
                        if query_date < from {
                            continue;
                        }
                    }
                }
                if let Some(ref vt) = triple.valid_to {
                    if let Ok(to) = chrono::DateTime::parse_from_rfc3339(vt)
                        .map(|d| d.with_timezone(&Utc))
                        .or_else(|_| {
                            chrono::NaiveDate::parse_from_str(vt, "%Y-%m-%d")
                                .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
                        })
                    {
                        if query_date > to {
                            continue;
                        }
                    }
                }
            }
            // If date_str can't be parsed, skip temporal filtering (include all)
        }

        results.push(triple);
    }

    if results.is_empty() {
        return ToolResult::success(format!("No knowledge graph triples found for entity '{}'.", entity));
    }

    let output_vals: Vec<Value> = results
        .iter()
        .map(|t| {
            json!({
                "subject": t.subject,
                "predicate": t.predicate,
                "object": t.object,
                "valid_from": t.valid_from,
                "valid_to": t.valid_to,
                "confidence": t.confidence,
                "created_at": t.created_at,
            })
        })
        .collect();

    let output = serde_json::to_string_pretty(&output_vals).unwrap_or_else(|e| format!("Serialization error: {}", e));
    ToolResult::success(format!("Found {} triple(s) for '{}':\n{}", results.len(), entity, output))
}

fn action_kg_invalidate(input: &Value, ctx: &ToolContext) -> ToolResult {
    let subject = match input.get("subject").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return ToolResult::error("Missing required parameter: subject"),
    };
    let predicate = match input.get("predicate").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return ToolResult::error("Missing required parameter: predicate"),
    };
    let object = match input.get("object").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return ToolResult::error("Missing required parameter: object"),
    };

    let kg = load_kg(ctx);
    let now = now_iso();
    let mut count = 0usize;

    let new_triples: Vec<KgTriple> = kg
        .triples
        .into_iter()
        .map(|mut t| {
            if t.subject.to_lowercase() == subject.to_lowercase()
                && t.predicate.to_lowercase() == predicate.to_lowercase()
                && t.object.to_lowercase() == object.to_lowercase()
                && t.valid_to.is_none()
            {
                t.valid_to = Some(now.clone());
                count += 1;
            }
            t
        })
        .collect();

    let updated_kg = KnowledgeGraph { triples: new_triples };
    if let Err(e) = save_kg(ctx, &updated_kg) {
        return ToolResult::error(format!("Failed to save knowledge graph: {}", e));
    }

    if count == 0 {
        ToolResult::success("No matching active triples found to invalidate.")
    } else {
        ToolResult::success(format!("Invalidated {} triple(s) ({} --[{}]--> {}).", count, subject, predicate, object))
    }
}

fn action_extract_memories(input: &Value, ctx: &ToolContext) -> ToolResult {
    let content = match input.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return ToolResult::error("Missing required parameter: content"),
    };
    let wing = match input.get("wing").and_then(|v| v.as_str()) {
        Some(w) => w,
        None => return ToolResult::error("Missing required parameter: wing"),
    };
    let room = match input.get("room").and_then(|v| v.as_str()) {
        Some(r) => r,
        None => return ToolResult::error("Missing required parameter: room"),
    };

    // Content size cap
    if content.len() > 65_536 {
        return ToolResult::error("Content exceeds 64 KB limit. Split into smaller chunks.");
    }

    let segments = extract_memory_segments(content);
    if segments.is_empty() {
        return ToolResult::success("No memory-worthy segments extracted from the provided content.");
    }

    let mut filed = Vec::new();
    for (segment, memory_type) in &segments {
        let drawer_input = json!({
            "wing": wing,
            "room": room,
            "content": segment,
            "memory_type": memory_type,
            "source": "auto-extracted",
        });
        let result = action_add_drawer(&drawer_input, ctx);
        if !result.is_error {
            filed.push(format!("  [{}] {}", memory_type, truncate_str(segment, 80)));
        }
    }

    ToolResult::success(format!(
        "Extracted and filed {} memory segment(s):\n{}",
        filed.len(),
        filed.join("\n")
    ))
}

fn action_get_layers(input: &Value, ctx: &ToolContext) -> ToolResult {
    let layer = match input.get("layer").and_then(|v| v.as_u64()) {
        Some(l) => l,
        None => return ToolResult::error("Missing required parameter: layer (0-3)"),
    };

    match layer {
        0 => {
            // L0: identity.txt content
            let id_path = identity_path(ctx);
            let content = std::fs::read_to_string(&id_path)
                .unwrap_or_else(|_| "No identity file found. Use 'init' to create the palace.".to_string());
            ToolResult::success(format!("=== L0: Identity ===\n{}", content))
        }
        1 => {
            // L1: Top drawers by recency across all wings (~500 tokens)
            let mut drawers = all_drawers(ctx);
            drawers.sort_by(|a, b| b.filed_at.cmp(&a.filed_at));
            drawers.truncate(10); // ~500 tokens

            if drawers.is_empty() {
                return ToolResult::success("=== L1: Recent Memories ===\nNo drawers found.");
            }

            let mut output = "=== L1: Recent Memories ===\n".to_string();
            for d in &drawers {
                output.push_str(&format!(
                    "[{}/{}] ({}) {}\n",
                    d.wing, d.room, d.memory_type, truncate_str(&d.content, 100)
                ));
            }
            ToolResult::success(output)
        }
        2 => {
            // L2: Drawers from a specific wing
            let wing = match input.get("wing").and_then(|v| v.as_str()) {
                Some(w) => w,
                None => return ToolResult::error("L2 requires 'wing' parameter"),
            };
            let mut drawers = drawers_in_wing(ctx, wing);
            drawers.sort_by(|a, b| b.filed_at.cmp(&a.filed_at));

            if drawers.is_empty() {
                return ToolResult::success(format!(
                    "=== L2: Wing '{}' ===\nNo drawers found.", sanitize_name(wing)
                ));
            }

            let mut output = format!("=== L2: Wing '{}' ({} drawers) ===\n", sanitize_name(wing), drawers.len());
            for d in &drawers {
                output.push_str(&format!(
                    "[{}] ({}) {}\n",
                    d.room, d.memory_type, truncate_str(&d.content, 120)
                ));
            }
            ToolResult::success(output)
        }
        3 => {
            // L3: Full search results
            let query = match input.get("query").and_then(|v| v.as_str()) {
                Some(q) => q,
                None => return ToolResult::error("L3 requires 'query' parameter"),
            };
            // Delegate to search
            let search_input = json!({
                "query": query,
                "max_results": 10,
            });
            let result = action_search(&search_input, ctx);
            let content = format!("=== L3: Search Results ===\n{}", result.content);
            ToolResult::success(content)
        }
        _ => ToolResult::error("Invalid layer. Must be 0, 1, 2, or 3."),
    }
}

fn action_delete_drawer(input: &Value, ctx: &ToolContext) -> ToolResult {
    let drawer_id = match input.get("drawer_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return ToolResult::error("Missing required parameter: drawer_id"),
    };

    // Search all drawers to find the matching one
    let all = all_drawers(ctx);
    let found = all.iter().find(|d| d.id == drawer_id);

    match found {
        Some(drawer) => {
            // Re-sanitize stored fields before path construction to prevent traversal
            let safe_id: String = drawer.id.chars()
                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            let path = room_dir(ctx, &drawer.wing, &drawer.room)
                .join(format!("{}.json", safe_id));

            // Verify the resolved path stays within the palace root
            let root = palace_root(ctx);
            if !path.starts_with(&root) {
                return ToolResult::error("Invalid drawer path: resolves outside palace directory.");
            }

            match std::fs::remove_file(&path) {
                Ok(_) => ToolResult::success(format!("Drawer '{}' deleted.", drawer_id)),
                Err(e) => ToolResult::error(format!("Failed to delete drawer file: {}", e)),
            }
        }
        None => ToolResult::error(format!("Drawer '{}' not found.", drawer_id)),
    }
}

/// Truncate a string to `max_len` characters, appending "..." if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len).collect();
        format!("{}...", truncated)
    }
}

// ---------------------------------------------------------------------------
// Tool implementation
// ---------------------------------------------------------------------------

pub struct MemPalaceTool;

#[async_trait]
impl Tool for MemPalaceTool {
    fn name(&self) -> &str {
        "MemPalace"
    }

    fn description(&self) -> &str {
        "Local-first memory system using the memory palace metaphor. Stores verbatim content \
         organized hierarchically: WING (project/person/topic) > ROOM (specific area) > DRAWER \
         (individual content chunk). Also maintains a Knowledge Graph for temporal entity \
         relationships (subject -> predicate -> object with validity windows).\n\n\
         Actions:\n\
         - init: Initialize palace structure. Optional 'wing' param.\n\
         - add_drawer: Store content. Params: wing, room, content, source?, memory_type? \
           (decision|preference|milestone|problem|emotional|general)\n\
         - search: Keyword search across drawers. Params: query, wing?, room?, max_results?\n\
         - list_wings: List all wings with drawer counts.\n\
         - list_rooms: List rooms in a wing. Params: wing\n\
         - status: Overall palace statistics.\n\
         - kg_add: Add a knowledge graph triple. Params: subject, predicate, object, valid_from?, \
           valid_to?, confidence?\n\
         - kg_query: Query knowledge graph. Params: entity, as_of?, direction? \
           (outgoing|incoming|both)\n\
         - kg_invalidate: Mark a triple as no longer valid. Params: subject, predicate, object\n\
         - extract_memories: Auto-extract decisions, preferences, milestones, problems, and \
           emotional moments from text. Params: content, wing, room\n\
         - get_layers: Return layered memory context. Params: layer (0=identity, 1=recent, \
           2=wing-specific, 3=search). Optional: wing (for L2), query (for L3)\n\
         - delete_drawer: Remove a drawer. Params: drawer_id\n\n\
         Storage: JSON files in {working_dir}/.claude/palace/"
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Write
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "init", "add_drawer", "search", "list_wings", "list_rooms",
                        "status", "kg_add", "kg_query", "kg_invalidate",
                        "extract_memories", "get_layers", "delete_drawer"
                    ],
                    "description": "The operation to perform."
                },
                "wing": {
                    "type": "string",
                    "description": "Wing name (project/person/topic)."
                },
                "room": {
                    "type": "string",
                    "description": "Room name (specific area within a wing)."
                },
                "content": {
                    "type": "string",
                    "description": "Content to store or extract memories from."
                },
                "source": {
                    "type": "string",
                    "description": "Optional source attribution for the content."
                },
                "memory_type": {
                    "type": "string",
                    "enum": ["decision", "preference", "milestone", "problem", "emotional", "general"],
                    "description": "Type classification for the memory."
                },
                "query": {
                    "type": "string",
                    "description": "Search query string."
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of search results (default 5)."
                },
                "subject": {
                    "type": "string",
                    "description": "Subject entity for knowledge graph operations."
                },
                "predicate": {
                    "type": "string",
                    "description": "Predicate (relationship) for knowledge graph operations."
                },
                "object": {
                    "type": "string",
                    "description": "Object entity for knowledge graph operations."
                },
                "valid_from": {
                    "type": "string",
                    "description": "Start of validity window (ISO 8601 date string)."
                },
                "valid_to": {
                    "type": "string",
                    "description": "End of validity window (ISO 8601 date string)."
                },
                "confidence": {
                    "type": "number",
                    "description": "Confidence score for KG triple (0.0 to 1.0, default 1.0)."
                },
                "entity": {
                    "type": "string",
                    "description": "Entity to query in the knowledge graph."
                },
                "as_of": {
                    "type": "string",
                    "description": "Date to filter KG triples by temporal validity."
                },
                "direction": {
                    "type": "string",
                    "enum": ["outgoing", "incoming", "both"],
                    "description": "Direction for KG query (default: both)."
                },
                "layer": {
                    "type": "integer",
                    "description": "Memory layer to retrieve (0=identity, 1=recent, 2=wing, 3=search)."
                },
                "drawer_id": {
                    "type": "string",
                    "description": "ID of the drawer to delete."
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let action = match input.get("action").and_then(|v| v.as_str()) {
            Some(a) => a.to_string(),
            None => return ToolResult::error("Missing required parameter: action"),
        };

        debug!(action = %action, "MemPalace action");

        match action.as_str() {
            "init" => action_init(&input, ctx),
            "add_drawer" => action_add_drawer(&input, ctx),
            "search" => action_search(&input, ctx),
            "list_wings" => action_list_wings(ctx),
            "list_rooms" => action_list_rooms(&input, ctx),
            "status" => action_status(ctx),
            "kg_add" => action_kg_add(&input, ctx),
            "kg_query" => action_kg_query(&input, ctx),
            "kg_invalidate" => action_kg_invalidate(&input, ctx),
            "extract_memories" => action_extract_memories(&input, ctx),
            "get_layers" => action_get_layers(&input, ctx),
            "delete_drawer" => action_delete_drawer(&input, ctx),
            other => ToolResult::error(format!(
                "Unknown action '{}'. Valid actions: init, add_drawer, search, list_wings, \
                 list_rooms, status, kg_add, kg_query, kg_invalidate, extract_memories, \
                 get_layers, delete_drawer",
                other
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_hash_deterministic() {
        let h1 = simple_hash("hello world");
        let h2 = simple_hash("hello world");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 8);
    }

    #[test]
    fn test_simple_hash_different_inputs() {
        let h1 = simple_hash("foo");
        let h2 = simple_hash("bar");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("My Project!"), "my_project_");
        assert_eq!(sanitize_name("hello-world"), "hello-world");
        assert_eq!(sanitize_name("CAPS_123"), "caps_123");
        assert_eq!(sanitize_name("spaces here"), "spaces_here");
    }

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("Hello, World! This is a test.");
        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
        assert!(tokens.contains(&"this".to_string()));
        assert!(tokens.contains(&"test".to_string()));
        // Single-char tokens filtered out
        assert!(!tokens.contains(&"a".to_string()));
    }

    #[test]
    fn test_keyword_score_match() {
        let query = tokenize("rust programming");
        let score = keyword_score(&query, "I love rust programming and systems development");
        assert!(score > 0.0);
    }

    #[test]
    fn test_keyword_score_no_match() {
        let query = tokenize("python django");
        let score = keyword_score(&query, "I love rust programming");
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_extract_memory_segments_decisions() {
        let content = "We decided to use Rust for the backend. The weather is nice today.";
        let segments = extract_memory_segments(content);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].1, "decision");
    }

    #[test]
    fn test_extract_memory_segments_multiple() {
        let content = "We decided to use Rust. Got it working finally! There was a bug in the parser.";
        let segments = extract_memory_segments(content);
        assert_eq!(segments.len(), 3);
        let types: Vec<&str> = segments.iter().map(|s| s.1.as_str()).collect();
        assert!(types.contains(&"decision"));
        assert!(types.contains(&"milestone"));
        assert!(types.contains(&"problem"));
    }

    #[test]
    fn test_extract_memory_segments_none() {
        let content = "The sky is blue and water is wet.";
        let segments = extract_memory_segments(content);
        assert!(segments.is_empty());
    }

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        let result = truncate_str("hello world this is long", 11);
        assert_eq!(result, "hello world...");
    }

    #[test]
    fn test_tool_name() {
        let tool = MemPalaceTool;
        assert_eq!(tool.name(), "MemPalace");
    }

    #[test]
    fn test_tool_permission_level() {
        let tool = MemPalaceTool;
        assert_eq!(tool.permission_level(), PermissionLevel::Write);
    }

    #[test]
    fn test_tool_input_schema_valid() {
        let tool = MemPalaceTool;
        let schema = tool.input_schema();
        assert!(schema.is_object());
        assert!(schema.get("properties").is_some());
        assert!(schema.get("required").is_some());
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "action");
    }

    #[test]
    fn test_default_memory_type() {
        assert_eq!(default_memory_type(), "general");
    }

    #[test]
    fn test_default_confidence() {
        assert_eq!(default_confidence(), 1.0);
    }
}
