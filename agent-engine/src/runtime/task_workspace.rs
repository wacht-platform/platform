use crate::filesystem::AgentFilesystem;
use chrono::{DateTime, Utc};
use common::error::AppError;
use sha2::{Digest, Sha256};

pub const TASK_WORKSPACE_DIR: &str = "/task";
pub const TASK_WORKSPACE_TASK_FILE: &str = "/task/TASK.md";
pub const TASK_WORKSPACE_JOURNAL_FILE: &str = "/task/JOURNAL.md";
pub const TASK_WORKSPACE_AUDIT_DIR: &str = "/task/audit";

const JOURNAL_TAIL_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct HandoffPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub findings: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cautions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

/// Idempotent on retry via the `<!-- assignment:N -->` marker — second
/// call with the same `assignment_id` returns `Ok(false)` without writing.
pub async fn append_journal_handoff_entry(
    filesystem: &AgentFilesystem,
    assignment_id: i64,
    agent_name: &str,
    final_status: &str,
    outcome: &str,
    handoff: &HandoffPayload,
    artifacts: &[String],
    timestamp: DateTime<Utc>,
) -> Result<bool, AppError> {
    let existing = read_sandbox_optional(filesystem, TASK_WORKSPACE_JOURNAL_FILE)
        .await?
        .unwrap_or_default();
    let marker = assignment_marker(assignment_id);
    if existing
        .windows(marker.len())
        .any(|window| window == marker.as_bytes())
    {
        return Ok(false);
    }

    let mut entry = String::new();
    let needs_leading_newline = !existing.is_empty() && !existing.ends_with(b"\n");
    if needs_leading_newline {
        entry.push('\n');
    }
    if !existing.is_empty() {
        entry.push('\n');
    }
    entry.push_str(&format!(
        "## {} · {} · {}\n",
        timestamp.format("%Y-%m-%dT%H:%M:%SZ"),
        agent_name,
        final_status,
    ));
    let trimmed_outcome = outcome.trim();
    if !trimmed_outcome.is_empty() {
        entry.push_str("outcome: ");
        // Multi-line outcomes would silently drop trailing lines on parse.
        entry.push_str(&collapse_to_single_line(trimmed_outcome));
        entry.push('\n');
    }
    if let Some(line) = handoff_line(&handoff.findings) {
        entry.push_str("findings: ");
        entry.push_str(&line);
        entry.push('\n');
    }
    if let Some(line) = handoff_line(&handoff.cautions) {
        entry.push_str("cautions: ");
        entry.push_str(&line);
        entry.push('\n');
    }
    if !artifacts.is_empty() {
        entry.push_str("artifacts: ");
        entry.push_str(&artifacts.join(", "));
        entry.push('\n');
    }
    if let Some(line) = handoff_line(&handoff.next) {
        entry.push_str("next: ");
        entry.push_str(&line);
        entry.push('\n');
    }
    entry.push_str(&marker);
    entry.push('\n');

    let mut combined = existing;
    combined.extend_from_slice(entry.as_bytes());
    filesystem
        .write_file(
            TASK_WORKSPACE_JOURNAL_FILE,
            &String::from_utf8_lossy(&combined),
            false,
        )
        .await?;
    Ok(true)
}

fn handoff_line(field: &Option<String>) -> Option<String> {
    let raw = field.as_deref()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

fn assignment_marker(assignment_id: i64) -> String {
    format!("<!-- assignment:{assignment_id} -->")
}

fn collapse_to_single_line(value: &str) -> String {
    let normalised = value.replace("\r\n", "\n").replace('\r', "\n");
    let joined = normalised
        .split('\n')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" | ");
    joined.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub struct TaskWorkspaceBriefInput<'a> {
    pub task_key: &'a str,
    pub title: &'a str,
    pub brief: Option<&'a str>,
}

pub struct PreparedTaskWorkspace {
    pub journal_hash: String,
}

async fn read_sandbox_optional(
    filesystem: &AgentFilesystem,
    path: &str,
) -> Result<Option<Vec<u8>>, AppError> {
    match filesystem.read_file_bytes(path).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(AppError::NotFound(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

pub async fn prepare_task_workspace(
    filesystem: &AgentFilesystem,
    input: &TaskWorkspaceBriefInput<'_>,
) -> Result<PreparedTaskWorkspace, AppError> {
    let task_key = input.task_key;

    let journal_bytes = match read_sandbox_optional(filesystem, TASK_WORKSPACE_JOURNAL_FILE).await?
    {
        Some(bytes) => bytes,
        None => {
            let contents = format!(
                "# Journal\n\nTask key: {}\nTask title: {}\n\nThis file is the append-only task history. Update it with durable handoff context before you finish a task stage.\n",
                task_key, input.title
            );
            filesystem
                .write_file(TASK_WORKSPACE_JOURNAL_FILE, &contents, false)
                .await?;
            contents.into_bytes()
        }
    };

    if let Some(brief) = input.brief {
        if read_sandbox_optional(filesystem, TASK_WORKSPACE_TASK_FILE)
            .await?
            .is_none()
        {
            let contents = format!("# {}\n\n{}\n", input.title, brief);
            filesystem
                .write_file(TASK_WORKSPACE_TASK_FILE, &contents, false)
                .await?;
        }
    }

    Ok(PreparedTaskWorkspace {
        journal_hash: hash_bytes(&journal_bytes),
    })
}

pub async fn compute_task_journal_hash(filesystem: &AgentFilesystem) -> Result<String, AppError> {
    let bytes = read_sandbox_optional(filesystem, TASK_WORKSPACE_JOURNAL_FILE)
        .await?
        .unwrap_or_default();
    Ok(hash_bytes(&bytes))
}

pub async fn read_task_journal_tail(
    filesystem: &AgentFilesystem,
) -> Result<Option<Vec<u8>>, AppError> {
    let Some(bytes) = read_sandbox_optional(filesystem, TASK_WORKSPACE_JOURNAL_FILE).await? else {
        return Ok(None);
    };
    if bytes.len() <= JOURNAL_TAIL_BYTES {
        Ok(Some(bytes))
    } else {
        let start = bytes.len() - JOURNAL_TAIL_BYTES;
        Ok(Some(bytes[start..].to_vec()))
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{:02x}", byte);
    }
    encoded
}

pub const JOURNAL_COMPACTION_THRESHOLD_BYTES: usize = 64 * 1024;
pub const JOURNAL_RECENT_ENTRIES_KEPT: usize = 20;
pub const JOURNAL_MAX_CHECKPOINTS_BEFORE_MERGE: usize = 4;

#[derive(Debug, Clone)]
pub struct JournalEntry {
    pub timestamp: String,
    pub actor: String,
    pub assignment_id: Option<i64>,
    pub outcome: Option<String>,
    pub cautions: Option<String>,
    pub artifacts: Vec<String>,
    /// Verbatim entry text; reused when the tail is preserved through compaction.
    pub raw: String,
}

#[derive(Debug, Clone)]
pub struct JournalCheckpoint {
    pub raw: String,
}

impl JournalCheckpoint {
    fn section_bullets(&self, header: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut in_section = false;
        for line in self.raw.lines() {
            let trimmed = line.trim_end();
            if trimmed.starts_with("### ") {
                in_section = trimmed[4..].trim_start().starts_with(header);
                continue;
            }
            if !in_section {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("- ") {
                let value = rest.trim();
                if !value.is_empty() {
                    out.push(value.to_string());
                }
            }
        }
        out
    }

    pub fn cautions(&self) -> Vec<String> {
        self.section_bullets("Cautions")
    }

    pub fn artifacts(&self) -> Vec<String> {
        self.section_bullets("Artifacts")
    }

    /// Returns Earlier-eras + Activity-by-actor sections combined, with
    /// headers preserved so recursive meta-merge round-trips losslessly.
    /// Cautions / artifacts are excluded; they're deduped separately.
    pub fn activity_body(&self) -> String {
        let mut out = String::new();
        let mut in_relevant = false;
        for line in self.raw.lines() {
            let trimmed = line.trim_end();
            if trimmed.starts_with("<!-- checkpoint:") {
                break;
            }
            if let Some(rest) = trimmed.strip_prefix("### ") {
                let header = rest.trim_start();
                in_relevant =
                    header.starts_with("Earlier eras") || header.starts_with("Activity by actor");
                if in_relevant {
                    if !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(trimmed);
                    out.push('\n');
                }
                continue;
            }
            if in_relevant {
                out.push_str(trimmed);
                out.push('\n');
            }
        }
        out.trim_end().to_string()
    }

    pub fn era_label(&self) -> String {
        self.raw
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("## Checkpoint · "))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "checkpoint".to_string())
    }
}

#[derive(Debug, Clone)]
pub enum JournalSection {
    Entry(JournalEntry),
    Checkpoint(JournalCheckpoint),
    Preamble(String),
}

pub fn parse_journal(content: &str) -> Vec<JournalSection> {
    let mut sections = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut in_section = false;

    for line in content.lines() {
        let starts_section = line.starts_with("## ");
        if starts_section && (in_section || !current.is_empty()) {
            flush_section(&mut sections, &current, in_section);
            current.clear();
        }
        if starts_section {
            in_section = true;
        }
        current.push(line);
    }
    if !current.is_empty() {
        flush_section(&mut sections, &current, in_section);
    }
    sections
}

fn flush_section(out: &mut Vec<JournalSection>, lines: &[&str], in_section: bool) {
    let raw = lines.join("\n");
    if !in_section {
        if !raw.trim().is_empty() {
            out.push(JournalSection::Preamble(raw));
        }
        return;
    }
    let header = lines[0];
    let body = header.strip_prefix("## ").unwrap_or(header).trim();
    if body.starts_with("Checkpoint") {
        out.push(JournalSection::Checkpoint(JournalCheckpoint { raw }));
        return;
    }
    if let Some(entry) = parse_entry(lines, raw.clone()) {
        out.push(JournalSection::Entry(entry));
    } else {
        // Unrecognised header: keep as preamble so round-trip is lossless.
        out.push(JournalSection::Preamble(raw));
    }
}

fn parse_entry(lines: &[&str], raw: String) -> Option<JournalEntry> {
    let header = lines.first()?.strip_prefix("## ")?.trim();
    let mut parts = header.split(" · ").map(str::trim);
    let timestamp = parts.next()?.to_string();
    let actor = parts.next()?.to_string();
    if timestamp.is_empty() || actor.is_empty() {
        return None;
    }

    let mut entry = JournalEntry {
        timestamp,
        actor,
        assignment_id: None,
        outcome: None,
        cautions: None,
        artifacts: Vec::new(),
        raw,
    };
    for line in lines.iter().skip(1) {
        let trimmed = line.trim_end();
        if let Some(rest) = trimmed.strip_prefix("outcome: ") {
            entry.outcome = Some(rest.to_string());
        } else if let Some(rest) = trimmed.strip_prefix("cautions: ") {
            entry.cautions = Some(rest.to_string());
        } else if let Some(rest) = trimmed.strip_prefix("artifacts: ") {
            entry.artifacts = rest
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        } else if let Some(rest) = trimmed.strip_prefix("<!-- assignment:") {
            if let Some(num) = rest.strip_suffix(" -->") {
                entry.assignment_id = num.trim().parse().ok();
            }
        }
        // findings: and next: are intentionally dropped on compaction.
    }
    Some(entry)
}

#[derive(Debug, Clone)]
pub struct CheckpointInputs {
    pub range_label: String,
    pub compacted_at: DateTime<Utc>,
    pub cautions: Vec<String>,
    pub artifacts: Vec<String>,
    /// `(turn_index_in_window, timestamp, outcome)` per actor.
    pub outcomes_by_actor: Vec<(String, Vec<(usize, String, String)>)>,
    /// `(era_label, activity_body)`. Populated only on meta-merge;
    /// rendered verbatim — never re-summarised.
    pub prior_eras: Vec<(String, String)>,
    head_kept: usize,
    tail_kept: usize,
}

impl CheckpointInputs {
    pub fn compacted_turn_count(&self) -> usize {
        self.outcomes_by_actor
            .iter()
            .map(|(_, list)| list.len())
            .sum()
    }
}

pub fn prepare_journal_compaction(content: &str) -> Option<CheckpointInputs> {
    if content.len() < JOURNAL_COMPACTION_THRESHOLD_BYTES {
        return None;
    }
    let sections = parse_journal(content);

    let total_checkpoints = sections
        .iter()
        .filter(|s| matches!(s, JournalSection::Checkpoint(_)))
        .count();
    let is_meta_merge = total_checkpoints >= JOURNAL_MAX_CHECKPOINTS_BEFORE_MERGE;

    let head_kept = if is_meta_merge {
        sections
            .iter()
            .position(|s| !matches!(s, JournalSection::Preamble(_)))
            .unwrap_or(sections.len())
    } else {
        sections
            .iter()
            .rposition(|s| matches!(s, JournalSection::Checkpoint(_)))
            .map(|idx| idx + 1)
            .unwrap_or_else(|| {
                sections
                    .iter()
                    .position(|s| matches!(s, JournalSection::Entry(_)))
                    .unwrap_or(sections.len())
            })
    };

    let remaining: Vec<&JournalSection> = sections.iter().skip(head_kept).collect();
    let entry_count = remaining
        .iter()
        .filter(|s| matches!(s, JournalSection::Entry(_)))
        .count();
    if entry_count <= JOURNAL_RECENT_ENTRIES_KEPT && !is_meta_merge {
        return None;
    }
    let compact_count = entry_count.saturating_sub(JOURNAL_RECENT_ENTRIES_KEPT);
    let tail_kept = JOURNAL_RECENT_ENTRIES_KEPT;

    let mut seen_cautions: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut seen_artifacts: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut cautions = Vec::new();
    let mut artifacts = Vec::new();
    let mut prior_eras: Vec<(String, String)> = Vec::new();

    if is_meta_merge {
        for section in sections.iter() {
            if let JournalSection::Checkpoint(cp) = section {
                for caution in cp.cautions() {
                    if seen_cautions.insert(caution.clone()) {
                        cautions.push(caution);
                    }
                }
                for artifact in cp.artifacts() {
                    if seen_artifacts.insert(artifact.clone()) {
                        artifacts.push(artifact);
                    }
                }
                let activity = cp.activity_body();
                if !activity.trim().is_empty() {
                    prior_eras.push((cp.era_label(), activity));
                }
            }
        }
    } else {
        for section in sections.iter().take(head_kept) {
            if let JournalSection::Checkpoint(cp) = section {
                for caution in cp.cautions() {
                    seen_cautions.insert(caution);
                }
                for artifact in cp.artifacts() {
                    seen_artifacts.insert(artifact);
                }
            }
        }
    }

    let mut by_actor: std::collections::BTreeMap<String, Vec<(usize, String, String)>> =
        std::collections::BTreeMap::new();

    let mut taken = 0usize;
    for section in remaining.iter() {
        if taken >= compact_count {
            break;
        }
        if let JournalSection::Entry(entry) = section {
            taken += 1;
            if let Some(c) = entry
                .cautions
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let line = format!("[{}] {}", entry.actor, c);
                if seen_cautions.insert(line.clone()) {
                    cautions.push(line);
                }
            }
            for art in &entry.artifacts {
                let trimmed = art.trim().to_string();
                if !trimmed.is_empty() && seen_artifacts.insert(trimmed.clone()) {
                    artifacts.push(trimmed);
                }
            }
            if let Some(outcome) = entry
                .outcome
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                by_actor.entry(entry.actor.clone()).or_default().push((
                    taken,
                    entry.timestamp.clone(),
                    outcome.to_string(),
                ));
            }
        }
    }

    let range_label = if is_meta_merge {
        format!(
            "merged {} prior eras + {} fresh entries",
            prior_eras.len(),
            taken
        )
    } else {
        format!("compacted {} entries", taken)
    };
    Some(CheckpointInputs {
        range_label,
        compacted_at: Utc::now(),
        cautions,
        artifacts,
        outcomes_by_actor: by_actor.into_iter().collect(),
        prior_eras,
        head_kept,
        tail_kept,
    })
}

pub fn render_rule_only_activity(inputs: &CheckpointInputs) -> String {
    let mut out = String::new();
    for (actor, turns) in &inputs.outcomes_by_actor {
        out.push_str(&format!("- **{}**\n", actor));
        for (_, ts, text) in turns {
            out.push_str(&format!("  - {ts}: {text}\n"));
        }
    }
    out
}

pub fn finalize_journal_compaction(
    original_content: &str,
    inputs: &CheckpointInputs,
    activity_summary: &str,
) -> String {
    let sections = parse_journal(original_content);
    let mut out = String::new();

    for section in sections.iter().take(inputs.head_kept) {
        let raw = match section {
            JournalSection::Entry(e) => &e.raw,
            JournalSection::Checkpoint(c) => &c.raw,
            JournalSection::Preamble(p) => p,
        };
        if !out.is_empty() && !out.ends_with("\n\n") {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
        }
        out.push_str(raw);
    }

    if !out.is_empty() && !out.ends_with("\n\n") {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(&render_checkpoint_block(inputs, activity_summary));

    let remaining: Vec<&JournalSection> = sections.iter().skip(inputs.head_kept).collect();
    let entries_only: Vec<&JournalSection> = remaining
        .iter()
        .copied()
        .filter(|s| matches!(s, JournalSection::Entry(_)))
        .collect();
    let tail_start = entries_only.len().saturating_sub(inputs.tail_kept);
    for section in entries_only.iter().skip(tail_start) {
        if let JournalSection::Entry(entry) = section {
            if !out.is_empty() && !out.ends_with("\n\n") {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }
            out.push_str(&entry.raw);
        }
    }

    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn scrub_marker_collisions(text: &str) -> String {
    text.replace("<!-- checkpoint:", "<!- checkpoint:")
        .replace("<!-- assignment:", "<!- assignment:")
}

fn render_checkpoint_block(inputs: &CheckpointInputs, activity_summary: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "## Checkpoint · {} · compacted {}\n",
        inputs.range_label,
        inputs.compacted_at.format("%Y-%m-%dT%H:%M:%SZ"),
    ));
    if !inputs.cautions.is_empty() {
        out.push_str("\n### Cautions (verbatim — never lossy)\n");
        for caution in &inputs.cautions {
            out.push_str(&format!("- {caution}\n"));
        }
    }
    if !inputs.artifacts.is_empty() {
        out.push_str("\n### Artifacts (still on disk)\n");
        for artifact in &inputs.artifacts {
            out.push_str(&format!("- {artifact}\n"));
        }
    }
    if !inputs.prior_eras.is_empty() {
        out.push_str("\n### Earlier eras\n");
        for (label, body) in &inputs.prior_eras {
            out.push_str(&format!("\n**{label}**\n"));
            let trimmed = body.trim();
            if !trimmed.is_empty() {
                out.push_str(trimmed);
                if !trimmed.ends_with('\n') {
                    out.push('\n');
                }
            }
        }
    }
    let scrubbed = scrub_marker_collisions(activity_summary);
    let activity = scrubbed.trim();
    if !activity.is_empty() {
        let header = if inputs.prior_eras.is_empty() {
            "### Activity by actor\n"
        } else {
            "### Activity by actor (this era)\n"
        };
        out.push('\n');
        out.push_str(header);
        out.push_str(activity);
        if !activity.ends_with('\n') {
            out.push('\n');
        }
    }
    out.push_str(&format!(
        "\n<!-- checkpoint:{} -->\n",
        inputs.compacted_turn_count()
    ));
    out
}
