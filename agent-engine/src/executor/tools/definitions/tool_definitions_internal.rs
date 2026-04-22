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
            "Read an image file and return mime metadata plus base64 payload for one-time vision analysis.",
            InternalToolType::ReadImage,
            vec![SchemaField {
                name: "path".to_string(),
                field_type: "STRING".to_string(),
                description: Some("Path to image file (e.g. /uploads/photo.png)".to_string()),
                required: true,
                ..Default::default()
            }],
        ),
        (
            "read_file",
            "Read a text file and return numbered lines plus a slice_hash for an exact file slice. Use this before edit_file so the replacement range is grounded in the current file contents.",
            InternalToolType::ReadFile,
            vec![
                SchemaField {
                    name: "path".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Path to read. Use this to inspect the exact file or line range you plan to edit. In conversation threads, `/project_workspace/` is available for project-scoped shared inspection such as `/project_workspace/tasks/<task_key>/...`.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "start_line".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Optional starting line to read (1-indexed). Omit for the file start.".to_string()),
                    minimum: Some(1.0),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "end_line".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Optional ending line to read (inclusive). Omit for the file end.".to_string()),
                    minimum: Some(1.0),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "write_file",
            "Write text to a file. Prefer this for creating, overwriting, or appending artifacts, notes, reports, markdown, JSON, or any other multi-line content. Use `append: true` for simple end-of-file appends. Do not use execute_command with shell text-emission tricks, heredocs, or python -c for large text emission when write_file is available.",
            InternalToolType::WriteFile,
            vec![
                SchemaField {
                    name: "path".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Path to write. Use /workspace/ for conversation-thread files. `/project_workspace/` is read-only inspection context, not a write target. Use /task/ only when a project-task workspace is actually mounted for the current work.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "content".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Content to write".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "append".to_string(),
                    field_type: "BOOLEAN".to_string(),
                    description: Some("Append content to the end of the file instead of overwriting it. Default: false.".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "edit_file",
            "Edit an existing text file by replacing an explicit line range. Read the target range first with read_file, then provide the returned slice_hash as live_slice_hash for that same range. Runtime verifies the current slice before editing. Use this for targeted edits; use write_file for create, overwrite, or append flows.",
            InternalToolType::EditFile,
            vec![
                SchemaField {
                    name: "path".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Existing file path to edit. Use /workspace/ for conversation-thread files. `/project_workspace/` is read-only inspection context, not an edit target. Use /task/ only when a project-task workspace is actually mounted for the current work.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "new_content".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Replacement content for the specified line range.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "start_line".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Replace from this line (1-indexed). The range must already be covered by a prior read_file call.".to_string()),
                    minimum: Some(1.0),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "end_line".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Replace up to this line (inclusive). The range must already be covered by a prior read_file call.".to_string()),
                    minimum: Some(1.0),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "live_slice_hash".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("The slice_hash previously returned by read_file for this exact line range. Runtime verifies this before editing unless dangerously_skip_slice_comparison is true.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "dangerously_skip_slice_comparison".to_string(),
                    field_type: "BOOLEAN".to_string(),
                    description: Some("Dangerous escape hatch. Set true only when fully confident the edit range is correct and it is acceptable to skip prior read-window and live_slice_hash verification, for example on a very small file. Default: false.".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "execute_command",
            "Execute a shell command for inspection, filtering, discovery, or simple shell-native file operations. Do not use this to emit long markdown/JSON/text blobs when write_file is available. Non-zero exit codes are treated as tool errors. Allowed commands: cat, head, tail, grep, rg, find, ls, wc, mkdir, cp, mv, rm, sed, awk, sort, uniq, jq, cut, tr, diff, which, python, python3.",
            InternalToolType::ExecuteCommand,
            vec![SchemaField {
                name: "command".to_string(),
                field_type: "STRING".to_string(),
                description: Some("Shell command to run".to_string()),
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
                    description: Some("Sleep duration in milliseconds (max 10000).".to_string()),
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
            "Search the public web via Parallel Search. Use this first to discover relevant URLs and excerpts before extracting full page content.",
            InternalToolType::WebSearch,
            vec![
                SchemaField {
                    name: "objective".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Natural-language web research objective. Provide this or search_queries.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "search_queries".to_string(),
                    field_type: "ARRAY".to_string(),
                    items_type: Some("STRING".to_string()),
                    description: Some("Optional keyword queries that guide the search. Provide this or objective.".to_string()),
                    min_items: Some(1),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "mode".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Search preset: `agentic` for token-efficient loops, `one-shot` for broader excerpts, `fast` for lower latency. Default: agentic.".to_string()),
                    enum_values: string_enum(&["agentic", "one-shot", "fast"]),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "max_results".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Optional upper bound on returned results. Default: 10.".to_string()),
                    minimum: Some(1.0),
                    maximum: Some(50.0),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "include_domains".to_string(),
                    field_type: "ARRAY".to_string(),
                    items_type: Some("STRING".to_string()),
                    description: Some("Optional domains to include, such as `example.com` or `.gov`.".to_string()),
                    min_items: Some(1),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "exclude_domains".to_string(),
                    field_type: "ARRAY".to_string(),
                    items_type: Some("STRING".to_string()),
                    description: Some("Optional domains to exclude.".to_string()),
                    min_items: Some(1),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "after_date".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Optional YYYY-MM-DD freshness filter.".to_string()),
                    format: Some("date".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "excerpt_max_chars_per_result".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Optional cap for excerpt characters per result. Omit unless you need a smaller-than-default budget. Defaults to a tool-managed budget of about 50k tokens total across the response.".to_string()),
                    minimum: Some(1000.0),
                    maximum: Some(100000.0),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "excerpt_max_chars_total".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Optional total excerpt character cap. Omit unless you need a smaller-than-default budget. Defaults to about 200k chars (~50k tokens).".to_string()),
                    minimum: Some(1000.0),
                    maximum: Some(200000.0),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "url_content",
            "Fetch focused excerpts or full markdown content for one or more URLs via Parallel Extract. Use this after web_search when you need page-level evidence.",
            InternalToolType::UrlContent,
            vec![
                SchemaField {
                    name: "urls".to_string(),
                    field_type: "ARRAY".to_string(),
                    items_type: Some("STRING".to_string()),
                    description: Some("One or more URLs to extract content from.".to_string()),
                    min_items: Some(1),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "objective".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Optional extraction objective to focus the returned content.".to_string()),
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
                    description: Some("Include focused excerpts. Default: true.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "full_content".to_string(),
                    field_type: "BOOLEAN".to_string(),
                    description: Some("Include full markdown page content. Default: false.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "excerpt_max_chars_per_result".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Optional cap for excerpt characters per URL. Omit unless you need a smaller-than-default budget.".to_string()),
                    minimum: Some(1000.0),
                    maximum: Some(200000.0),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "excerpt_max_chars_total".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Optional total excerpt cap across all URLs. Omit unless you need a smaller-than-default budget. Defaults to about 200k chars (~50k tokens).".to_string()),
                    minimum: Some(1000.0),
                    maximum: Some(200000.0),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "full_content_max_chars_per_result".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Optional per-URL cap for full content. Omit unless you need a smaller-than-default budget. Defaults to a fair share of about 200k chars across the requested URLs.".to_string()),
                    minimum: Some(1000.0),
                    maximum: Some(200000.0),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "search_knowledgebase",
            "Search linked local knowledge bases and return typed candidate documents and chunks. Use when you need evidence from linked KBs.",
            InternalToolType::SearchKnowledgebase,
            vec![
                SchemaField {
                    name: "query".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Retrieval query to run against linked local knowledge bases.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "search_type".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Retrieval strategy. Default: semantic.".to_string()),
                    enum_values: string_enum(&["semantic", "keyword"]),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "knowledge_base_ids".to_string(),
                    field_type: "ARRAY".to_string(),
                    items_type: Some("STRING".to_string()),
                    description: Some("Optional KB IDs to scope the search. If omitted, all linked KBs are searched.".to_string()),
                    min_items: Some(1),
                    max_items: Some(10),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "max_results".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Max retrieved matches before post-processing. Default: 12.".to_string()),
                    minimum: Some(1.0),
                    maximum: Some(50.0),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "include_associated_chunks".to_string(),
                    field_type: "BOOLEAN".to_string(),
                    description: Some("Whether to load additional related chunks for top documents. Default: true.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "max_associated_chunks_per_document".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Additional chunks to load per top document when include_associated_chunks=true. Default: 3.".to_string()),
                    minimum: Some(1.0),
                    maximum: Some(10.0),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "search_tools",
            "Search the full external tool catalog by one or more natural-language descriptions. Returns the best matching tool names plus their input schemas and usage guidance.",
            InternalToolType::SearchTools,
            vec![
                SchemaField {
                    name: "queries".to_string(),
                    field_type: "ARRAY".to_string(),
                    description: Some("One or more natural-language descriptions of the tools you need.".to_string()),
                    required: true,
                    items_type: Some("STRING".to_string()),
                    min_items: Some(1),
                    ..Default::default()
                },
                SchemaField {
                    name: "max_results_per_query".to_string(),
                    field_type: "INTEGER".to_string(),
                    description: Some("Optional max number of matches to return for each query (default 3, max 5).".to_string()),
                    minimum: Some(1.0),
                    maximum: Some(5.0),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "load_tools",
            "Load one or more external tools by exact tool name. At most 10 external tools stay loaded; when exceeded, the oldest loaded tools are evicted automatically.",
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
            "Load long-term memory records by semantic search, full-text search, or hybrid search. Use for durable past state, facts, patterns, or prior IDs that matter now.",
            InternalToolType::LoadMemory,
            vec![
                SchemaField {
                    name: "query".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Memory query text. Leave empty to fetch recent matching memories from the selected sources.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "categories".to_string(),
                    field_type: "ARRAY".to_string(),
                    description: Some("Memory categories to include.".to_string()),
                    items_type: Some("STRING".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "sources".to_string(),
                    field_type: "ARRAY".to_string(),
                    description: Some("Memory scopes to search: thread, project, actor, or agent.".to_string()),
                    items_type: Some("STRING".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "depth".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Search depth: shallow, moderate, or deep.".to_string()),
                    enum_values: string_enum(&["shallow", "moderate", "deep"]),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "search_approach".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Retrieval approach: semantic, full_text, or hybrid.".to_string()),
                    enum_values: string_enum(&["semantic", "full_text", "hybrid"]),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "save_memory",
            "Save a durable fact or procedure. Use for things that will matter beyond the current task.",
            InternalToolType::SaveMemory,
            vec![
                SchemaField {
                    name: "content".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("The distilled rule or procedure. Three lines: the fact, `Why:` the reason, `How to apply:` the trigger.".to_string()),
                    required: true,
                    ..Default::default()
                },
                SchemaField {
                    name: "category".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Category: semantic (facts, decisions, constraints) or procedural (validated how-to). Defaults to semantic.".to_string()),
                    enum_values: string_enum(&["semantic", "procedural"]),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "scope".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Who can recall this. actor (user-wide), project (this project only), thread (this task lane only). Defaults to project.".to_string()),
                    enum_values: string_enum(&["actor", "project", "thread"]),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "observation".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("The scenario that led to the insight. Populate for non-trivial memories so future retrievals can reconstruct context.".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "signals".to_string(),
                    field_type: "ARRAY".to_string(),
                    description: Some("Short cue phrases (3-6 words each) that signal this memory is applicable.".to_string()),
                    items_type: Some("STRING".to_string()),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "related".to_string(),
                    field_type: "ARRAY".to_string(),
                    description: Some("Memory IDs of related entries forming the reasoning chain.".to_string()),
                    items_type: Some("STRING".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
        (
            "update_memory",
            "Correct or refine an existing memory in place. Use when the prior entry was wrong (bad fact, wrong category, stale location). For a rule that legitimately changed, save a new memory and link the old via `related` instead.",
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
                    description: Some("New content. Omit to keep the current value.".to_string()),
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
                    description: Some("Scope changes are not supported here; re-save the memory in the new scope instead.".to_string()),
                    enum_values: string_enum(&["actor", "project", "thread"]),
                    required: false,
                    ..Default::default()
                },
                SchemaField {
                    name: "observation".to_string(),
                    field_type: "STRING".to_string(),
                    description: Some("Replace the observation. Pass an empty string to clear it.".to_string()),
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
                    description: Some("Replace the related-memory-ids list. Empty array clears it.".to_string()),
                    items_type: Some("STRING".to_string()),
                    required: false,
                    ..Default::default()
                },
            ],
        ),
    ]
}
