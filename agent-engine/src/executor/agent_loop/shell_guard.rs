//! Shell-discipline classifier for `execute_command`. Never blocks — shell is
//! often the right tool. It only nudges toward the dedicated file tools when a
//! command does something they do better (writing file content, whole-file
//! `cat`). The loop escalates a repeated nudge into a reflect-and-switch steer.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShellVerdict {
    Allow,
    Nudge(String),
}

const TRACKED_WRITE_PREFIXES: [&str; 6] = [
    "/workspace",
    "/task",
    "/project_workspace",
    "/shared",
    "/delegated_workspace",
    "/uploads",
];

fn unquote(token: &str) -> &str {
    token.trim_matches('"').trim_matches('\'')
}

// Throwaway sinks and untracked system paths are left to the shell; tracked
// mounts and relative paths route through the file tools.
fn is_tracked_write_target(target: &str) -> bool {
    let t = unquote(target);
    if t.is_empty() {
        return false;
    }
    if t.starts_with("/tmp")
        || t.starts_with("/scratch")
        || t.starts_with("/dev/")
        || t.starts_with("/var/tmp")
        || t.starts_with("/run/")
    {
        return false;
    }
    if TRACKED_WRITE_PREFIXES
        .iter()
        .any(|p| t == *p || t.starts_with(&format!("{p}/")))
    {
        return true;
    }
    if t.starts_with('/') {
        return false;
    }
    true
}

// Redirect (`>`, `>>`, `&>`, `N>`) to a tracked target. Skips fd dups
// (`2>&1`, `>&2`) and `/dev` sinks. Quote-aware.
fn has_tracked_redirect(command: &str) -> bool {
    let bytes = command.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < len {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == b'\'' || c == b'"' {
            quote = Some(c);
            i += 1;
            continue;
        }
        if c == b'>' {
            let mut j = i + 1;
            if j < len && bytes[j] == b'>' {
                j += 1;
            }
            while j < len && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j < len && bytes[j] == b'&' {
                i = j + 1;
                continue;
            }
            let start = j;
            while j < len {
                let tc = bytes[j];
                if matches!(tc, b' ' | b'\t' | b'|' | b'&' | b';' | b'>' | b'<' | b'\n') {
                    break;
                }
                j += 1;
            }
            if start < j && is_tracked_write_target(&command[start..j]) {
                return true;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    false
}

// Split into pipeline/sequence segments on `|`, `&&`, `||`, `;`, newlines.
fn segments(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let bytes = command.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            cur.push(c as char);
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == b'\'' || c == b'"' {
            quote = Some(c);
            cur.push(c as char);
            i += 1;
            continue;
        }
        let two = if i + 1 < bytes.len() {
            &command[i..i + 2]
        } else {
            ""
        };
        if two == "&&" || two == "||" {
            out.push(std::mem::take(&mut cur));
            i += 2;
            continue;
        }
        if c == b'|' || c == b';' || c == b'\n' {
            out.push(std::mem::take(&mut cur));
            i += 1;
            continue;
        }
        cur.push(c as char);
        i += 1;
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Whitespace tokens of a segment, quotes stripped.
fn tokens(segment: &str) -> Vec<String> {
    segment
        .split_whitespace()
        .map(|t| unquote(t).to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// `sed -i` / `sed --in-place` (in-place file edit) in any form.
fn is_sed_inplace(toks: &[String]) -> bool {
    if toks.first().map(String::as_str) != Some("sed") {
        return false;
    }
    toks.iter().skip(1).any(|t| {
        if t == "--in-place" || t.starts_with("--in-place=") {
            return true;
        }
        if t.starts_with("--") {
            return false;
        }
        if let Some(rest) = t.strip_prefix('-') {
            // `-i` may sit anywhere in a short cluster, with an optional suffix.
            let cluster = rest.split(['.', '=']).next().unwrap_or("");
            return cluster.contains('i');
        }
        false
    })
}

/// `tee <tracked-file>` — writes file content through the shell.
fn is_tee_to_tracked(toks: &[String]) -> bool {
    if toks.first().map(String::as_str) != Some("tee") {
        return false;
    }
    toks.iter()
        .skip(1)
        .filter(|t| !t.starts_with('-'))
        .any(|t| is_tracked_write_target(t))
}

// Bare `cat <single file>`, no pipe/redirect. Piped `cat ... | grep` is fine.
fn is_bare_cat_read(command: &str, segs: &[String]) -> bool {
    if segs.len() != 1 || command.contains('>') {
        return false;
    }
    let toks = tokens(&segs[0]);
    if toks.first().map(String::as_str) != Some("cat") {
        return false;
    }
    let positionals: Vec<&String> = toks.iter().skip(1).filter(|t| !t.starts_with('-')).collect();
    if positionals.len() != 1 {
        return false;
    }
    let target = unquote(positionals[0]);
    if target.is_empty() || target.starts_with('<') || target.contains('$') {
        return false;
    }
    true
}

const NUDGE_WRITE_MSG: &str = "You wrote file content through the shell. Prefer `write_file` (create/overwrite), \
`append_file` (add lines), or `edit_file` (change a substring) — they honor read-before-edit and the trailing-newline \
guarantee that shell `>`/`>>`/`tee` skip. Shell stays great for inspection (`rg`, pipes, `find`).";

const NUDGE_SED_MSG: &str = "`sed -i` edits a file in place. Prefer `read_file` then `edit_file` (anchor `old_string` \
on the exact bytes you read) — keeps read-discipline intact. Shell stays great for inspection.";

const NUDGE_CAT_MSG: &str = "You used `cat` to read a whole file. Prefer `read_file`: it returns total_lines/total_chars and the \
`slice_hash` you need before `edit_file`, and pages large files cleanly. Reserve shell for filtering (`rg`, pipes) and paging windows.";

// Nudge beats Allow; never blocks.
pub(crate) fn classify_shell_command(command: &str) -> ShellVerdict {
    let command = command.trim();
    if command.is_empty() {
        return ShellVerdict::Allow;
    }

    if has_tracked_redirect(command) {
        return ShellVerdict::Nudge(NUDGE_WRITE_MSG.to_string());
    }

    let segs = segments(command);
    for seg in &segs {
        let toks = tokens(seg);
        if toks.is_empty() {
            continue;
        }
        if is_sed_inplace(&toks) {
            return ShellVerdict::Nudge(NUDGE_SED_MSG.to_string());
        }
        if is_tee_to_tracked(&toks) {
            return ShellVerdict::Nudge(NUDGE_WRITE_MSG.to_string());
        }
    }

    if is_bare_cat_read(command, &segs) {
        return ShellVerdict::Nudge(NUDGE_CAT_MSG.to_string());
    }

    ShellVerdict::Allow
}
