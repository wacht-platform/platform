use super::tool_definitions_common::string_enum;
use models::{InternalToolType, SchemaField};

pub(crate) fn internal_tools() -> Vec<(
    &'static str,
    &'static str,
    InternalToolType,
    Vec<SchemaField>,
)> {
    vec![
        (
            "read_image",
            "Read an image file; returns mime metadata + base64 for one-time vision analysis.",
            InternalToolType::ReadImage,
            vec![SchemaField {
                name: "path".to_string(),
                field_type: "STRING".to_string(),
                description: Some("Image file path (e.g. /uploads/photo.png).".to_string()),
                required: true,
                ..Default::default()
            }],
        ),
        (
            "read_file",
            "Read a text file. Required before any edit_file on the same path — the runtime rejects edits to files not read this turn. Copy old_string from this output.",
            InternalToolType::ReadFile,
            vec![
                SchemaField {
                    name: "path".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Path to read.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "start_line".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Optional start line (1-indexed). Omit for file start.".to_string()),
                    minimum: Some(1.0),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "end_line".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Optional end line (inclusive). Omit for file end.".to_string()),
                    minimum: Some(1.0),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "write_file",
            "Create or fully overwrite a file (always overwrites). Use append_file to add, edit_file to change a substring. Prefer over shell heredocs/python -c for multi-line text.",
            InternalToolType::WriteFile,
            vec![
                SchemaField {
                    name: "path".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Write path. Writeable mounts: /workspace/ (conversation), /task/ (task), /scratch/ (ephemeral).".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "content".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Full file contents; replaces any existing file.".to_string()),
                    required: true,
                    ..Default::default()
                },
            ],
        ),
        (
            "append_file",
            "Append to the end of a file (creates if missing). For journal/log lines and end-of-file additions. Newline separation from the tail and a trailing newline are inserted automatically — pass just your line(s). Use edit_file to change existing content; never use shell >> on tracked files (bypasses read-discipline + newline guarantee).",
            InternalToolType::AppendFile,
            vec![
                SchemaField {
                    name: "path".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Path to append to (created if missing). Writeable mounts: /workspace/, /task/, /scratch/.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "content".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Line(s) to append; newline separation is automatic.".to_string()),
                    required: true,
                    ..Default::default()
                },
            ],
        ),
        (
            "edit_file",
            "Replace an exact substring in a file. Anchor-based: old_string = exact bytes to find, new_string = replacement. Rules: (1) must have read_file'd the path this turn; (2) old_string must match exactly incl. whitespace/newlines — copy from read_file, don't paraphrase; (3) old_string must be unique unless replace_all=true (else the tool errors with the match count); (4) old_string non-empty and != new_string. Include 1-3 lines of surrounding context for uniqueness. For pure insertion, anchor on a nearby existing line. Use write_file to create/overwrite, append_file to add at end. Never edit files via shell (heredoc / > / >> / sed).",
            InternalToolType::EditFile,
            vec![
                SchemaField {
                    name: "path".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Existing file to edit. Writeable mount; must have been read this turn.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "old_string".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Exact bytes to find — whitespace/indent/newlines must match (copy from read_file). Unique unless replace_all=true.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "new_string".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Replacement bytes. Empty string deletes old_string. Must differ from old_string.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "replace_all".to_string(),
                    field_type: "BOOLEAN".to_string(),
                    description: Some("Replace every occurrence instead of requiring uniqueness. Default false.".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "execute_command",
            "Run a shell command in the sandbox (inspection, filtering, discovery, scripting, image/PDF tooling). Prefer write_file over piping long text through stdout. Returns exit_code/stdout/stderr — a non-zero exit is a normal shell signal to read and react to, not a platform error.",
            InternalToolType::ExecuteCommand,
            vec![SchemaField {
                name: "command".to_string(),
                field_type: "STRING".to_string(),
                description: Some("Shell command to run.".to_string()),
                required: true,
                ..Default::default()
            }],
        ),
        (
            "sleep",
            "Pause execution briefly. Use when waiting for external updates or when no immediate action is required.",
            InternalToolType::Sleep,
            vec![
                SchemaField {
                    name: "duration_ms".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Sleep duration in ms (max 10000).".to_string()),
                    minimum: Some(0.0),
                    maximum: Some(10000.0),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "reason".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Optional brief reason for the wait.".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "web_search",
            "Search the public web (Parallel Search). Use first to find URLs/excerpts before extracting full pages.",
            InternalToolType::WebSearch,
            vec![
                SchemaField {
                    name: "objective".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Web research objective. Provide this or search_queries.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "search_queries".to_string(),
                    field_type: "ARRAY".to_string(),
                    items_type: Some("STRING".to_string()),
                    description: Some("Keyword queries. Provide this or objective.".to_string()),
                    min_items: Some(1),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "mode".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("agentic (token-efficient, default), one-shot (broader excerpts), fast (low latency).".to_string()),
                    enum_values: string_enum(&["agentic", "one-shot", "fast"]),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "max_results".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Max results. Default 10.".to_string()),
                    minimum: Some(1.0),
                    maximum: Some(50.0),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "include_domains".to_string(),
                    field_type: "ARRAY".to_string(),
                    items_type: Some("STRING".to_string()),
                    description: Some("Domains to include (e.g. example.com, .gov).".to_string()),
                    min_items: Some(1),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "exclude_domains".to_string(),
                    field_type: "ARRAY".to_string(),
                    items_type: Some("STRING".to_string()),
                    description: Some("Domains to exclude.".to_string()),
                    min_items: Some(1),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "after_date".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("YYYY-MM-DD freshness filter.".to_string()),
                    format: Some("date".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "excerpt_max_chars_per_result".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Per-result excerpt char cap. Omit unless you need a smaller-than-default budget (~50k tokens total).".to_string()),
                    minimum: Some(1000.0),
                    maximum: Some(100000.0),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "excerpt_max_chars_total".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Total excerpt char cap. Omit unless smaller needed (default ~200k chars).".to_string()),
                    minimum: Some(1000.0),
                    maximum: Some(200000.0),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "url_content",
            "Fetch excerpts or full markdown for one or more URLs (Parallel Extract). Use after web_search for page-level evidence.",
            InternalToolType::UrlContent,
            vec![
                SchemaField {
                    name: "urls".to_string(),
                    field_type: "ARRAY".to_string(),
                    items_type: Some("STRING".to_string()),
                    description: Some("URLs to extract content from.".to_string()),
                    min_items: Some(1),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "objective".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Optional focus objective.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "search_queries".to_string(),
                    field_type: "ARRAY".to_string(),
                    items_type: Some("STRING".to_string()),
                    description: Some("Optional keyword queries to focus excerpts.".to_string()),
                    min_items: Some(1),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "excerpts".to_string(),
                    field_type: "BOOLEAN".to_string(),
                    description: Some("Include excerpts. Default true.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "full_content".to_string(),
                    field_type: "BOOLEAN".to_string(),
                    description: Some("Include full markdown. Default false.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "excerpt_max_chars_per_result".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Per-URL excerpt char cap. Omit unless smaller needed.".to_string()),
                    minimum: Some(1000.0),
                    maximum: Some(200000.0),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "excerpt_max_chars_total".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Total excerpt char cap across URLs. Omit unless smaller needed (default ~200k chars).".to_string()),
                    minimum: Some(1000.0),
                    maximum: Some(200000.0),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "full_content_max_chars_per_result".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Per-URL full-content char cap. Omit unless smaller needed.".to_string()),
                    minimum: Some(1000.0),
                    maximum: Some(200000.0),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "search_knowledgebase",
            "Search linked local knowledge bases; returns typed candidate documents and chunks. Use when you need evidence from linked KBs.",
            InternalToolType::SearchKnowledgebase,
            vec![
                SchemaField {
                    name: "query".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Retrieval query against linked KBs.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "search_type".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Retrieval strategy. Default semantic.".to_string()),
                    enum_values: string_enum(&["semantic", "keyword"]),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "knowledge_base_ids".to_string(),
                    field_type: "ARRAY".to_string(),
                    items_type: Some("STRING".to_string()),
                    description: Some("Optional KB IDs to scope the search. Omit to search all linked KBs.".to_string()),
                    min_items: Some(1),
                    max_items: Some(10),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "max_results".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Max matches before post-processing. Default 12.".to_string()),
                    minimum: Some(1.0),
                    maximum: Some(50.0),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "include_associated_chunks".to_string(),
                    field_type: "BOOLEAN".to_string(),
                    description: Some("Load related chunks for top documents. Default true.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "max_associated_chunks_per_document".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Related chunks per top document when enabled. Default 3.".to_string()),
                    minimum: Some(1.0),
                    maximum: Some(10.0),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "search_tools",
            "Discover external (virtual) tools across connected apps. Modes: search (default) ranks tools by `queries`; browse lists tools for specific `apps` (up to 100/service). FLOW: (1) call once per discovery need; (2) pick from the result (`recommended_tool_names` = top picks); (3) load_tools with that exact name; (4) call it directly. Loaded tools persist for the session. Don't re-search with similar queries to \"find more\" — the catalog is the same. Don't shell out (which / pip / composio / mcp) — these are runtime virtual tools, not installable binaries with a CLI.",
            InternalToolType::SearchTools,
            vec![
                SchemaField {
                    name: "queries".to_string(),
                    field_type: "ARRAY".to_string(),
                    description: Some("Natural-language descriptions of tools you need (e.g. \"send an email\"). Required for mode=search; ignored for mode=browse.".to_string()),
                    required: false,
                    items_type: Some("STRING".to_string()),
                    ..Default::default()
                },
                SchemaField {
                    name: "apps".to_string(),
                    field_type: "ARRAY".to_string(),
                    description: Some("App slugs to restrict to (e.g. [\"gmail\"]). Omit to search all connected apps. Strongly recommended for mode=browse — otherwise it auto-expands across all apps with a reduced per-app cap.".to_string()),
                    required: false,
                    items_type: Some("STRING".to_string()),
                    ..Default::default()
                },
                SchemaField {
                    name: "mode".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("search (default): rank by `queries`. browse: list featured tools for `apps` without keywords.".to_string()),
                    required: false,
                    enum_values: string_enum(&["search", "browse"]),
                    ..Default::default()
                },
                SchemaField {
                    name: "max_results_per_query".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Max matches per query. Defaults: 10 (max 25) search/unscoped browse; 100 (max 200) browse with explicit `apps`.".to_string()),
                    minimum: Some(1.0),
                    maximum: Some(200.0),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "load_tools",
            "Load external (virtual) tools by exact name (from a prior search_tools result) so they become directly callable. Invoke like any internal tool — there is no separate composio/mcp runtime to install. Up to 30 stay loaded; oldest evicted automatically.",
            InternalToolType::LoadTools,
            vec![SchemaField {
                name: "tool_names".to_string(),
                field_type: "ARRAY".to_string(),
                description: Some("Exact external tool names to load.".to_string()),
                required: true,
                items_type: Some("STRING".to_string()),
                min_items: Some(1),
                max_items: Some(10),
                ..Default::default()
            }],
        ),
        (
            "load_memory",
            "Load long-term memory by semantic, full-text, or hybrid search. Use for durable past state, facts, patterns, or prior IDs that matter now.",
            InternalToolType::LoadMemory,
            vec![
                SchemaField {
                    name: "query".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Query text. Empty fetches recent matches from the selected sources.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "categories".to_string(),
                    field_type: "ARRAY".to_string(),
                    description: Some("Categories to include.".to_string()),
                    items_type: Some("STRING".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "sources".to_string(),
                    field_type: "ARRAY".to_string(),
                    description: Some("Scopes to search: thread, project, actor, agent.".to_string()),
                    items_type: Some("STRING".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "depth".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Search depth.".to_string()),
                    enum_values: string_enum(&["shallow", "moderate", "deep"]),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "search_approach".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Retrieval approach.".to_string()),
                    enum_values: string_enum(&["semantic", "full_text", "hybrid"]),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "save_memory",
            "Save a durable fact or procedure that will matter beyond the current task.",
            InternalToolType::SaveMemory,
            vec![
                SchemaField {
                    name: "content".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("The distilled rule/procedure. Three lines: the fact, `Why:` the reason, `How to apply:` the trigger.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "category".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("semantic (facts/decisions/constraints) or procedural (validated how-to). Default semantic.".to_string()),
                    enum_values: string_enum(&["semantic", "procedural"]),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "scope".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Who can recall it: actor (user-wide), project, thread (this lane). Default project.".to_string()),
                    enum_values: string_enum(&["actor", "project", "thread"]),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "observation".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("The scenario that led to the insight. Populate for non-trivial memories.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "signals".to_string(),
                    field_type: "ARRAY".to_string(),
                    description: Some("Short cue phrases (3-6 words) signalling when this applies.".to_string()),
                    items_type: Some("STRING".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "related".to_string(),
                    field_type: "ARRAY".to_string(),
                    description: Some("Memory IDs of related entries in the reasoning chain.".to_string()),
                    items_type: Some("STRING".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "update_memory",
            "Correct or refine an existing memory in place (wrong fact, wrong category, stale location). For a rule that legitimately changed, save a new memory and link the old via `related` instead.",
            InternalToolType::UpdateMemory,
            vec![
                SchemaField {
                    name: "memory_id".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("ID of the memory to update.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "content".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("New content. Omit to keep current.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "category".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("New category. Omit to keep current.".to_string()),
                    enum_values: string_enum(&["semantic", "procedural"]),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "scope".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Scope changes not supported here; re-save in the new scope instead.".to_string()),
                    enum_values: string_enum(&["actor", "project", "thread"]),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "observation".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Replace the observation. Empty string clears it.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "signals".to_string(),
                    field_type: "ARRAY".to_string(),
                    description: Some("Replace the signals list. Empty array clears it.".to_string()),
                    items_type: Some("STRING".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "related".to_string(),
                    field_type: "ARRAY".to_string(),
                    description: Some("Replace the related-ids list. Empty array clears it.".to_string()),
                    items_type: Some("STRING".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
    ]
}
