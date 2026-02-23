// Validation and utility functions

use axum::http::HeaderMap;
use std::collections::{HashMap, HashSet};

/// Validate API key format
/// Expected format: {prefix}_{secret} where secret is exactly 43 chars (32 bytes base64-url-encoded)
/// The secret is always the last 43 characters of the key.
/// Examples: sk_live_abc123..., sk_test_xyz789...
pub fn is_valid_api_key_format(key: &str) -> bool {
    // Key must have at least: 2-char prefix + underscore + 43-char secret = 46 chars
    if key.len() < 46 {
        return false;
    }

    // The secret is always the last 43 characters
    let secret_part = match key.get(key.len() - 43..) {
        Some(s) => s,
        None => return false,
    };

    // Validate secret characters: alphanumeric, hyphen, or underscore (base64-url-safe)
    secret_part
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Validate permission expression syntax
/// Returns Ok(()) if valid, Err(error message) if invalid
pub fn validate_permission_expr(expr: &str) -> Result<(), String> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Err("empty expression".to_string());
    }

    let mut paren_depth = 0;
    let mut has_content = false;
    let mut i = 0;
    let chars: Vec<char> = expr.chars().collect();

    while i < chars.len() {
        match chars[i] {
            '(' => {
                paren_depth += 1;
                i += 1;
            }
            ')' => {
                paren_depth -= 1;
                if paren_depth < 0 {
                    return Err("unmatched closing parenthesis".to_string());
                }
                i += 1;
            }
            ' ' | '\t' => {
                i += 1;
            }
            _ => {
                // Check for invalid operators
                if i + 2 < chars.len() {
                    let two_chars = format!("{}{}", chars[i], chars[i + 1]);
                    if two_chars == "&&" || two_chars == "||" {
                        return Err(format!("invalid operator '{}', use AND/OR", two_chars));
                    }
                }
                has_content = true;
                i += 1;
            }
        }
    }

    if paren_depth != 0 {
        return Err("unmatched opening parenthesis".to_string());
    }

    if !has_content {
        return Err("expression contains no permissions".to_string());
    }

    // Check for trailing operators
    let expr_trimmed = expr.trim_end();
    if expr_trimmed.ends_with(" AND") || expr_trimmed.ends_with(" OR") {
        return Err("trailing operator".to_string());
    }

    // Check for leading operators
    let expr_trimmed = expr.trim_start();
    if expr_trimmed.starts_with(" AND ") || expr_trimmed.starts_with(" OR ") {
        return Err("leading operator".to_string());
    }

    Ok(())
}

/// Check if key permissions satisfy required permissions expression
/// Supports:
/// - "perm1,perm2" or "perm1 AND perm2" = must have ALL
/// - "perm1 OR perm2" = must have at least ONE
/// - "perm1 AND (perm2 OR perm3)" = complex expressions
pub fn check_permissions(key_permissions: &HashSet<&str>, required: &str) -> bool {
    let required = required.trim();
    if required.is_empty() {
        return true;
    }

    // Simple case: comma-separated = AND
    if !required.contains(" AND ") && !required.contains(" OR ") {
        let perms: Vec<&str> = required
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        return perms.iter().all(|p| key_permissions.contains(p));
    }

    // Complex case with AND/OR
    evaluate_permission_expr(key_permissions, required)
}

/// Evaluate a permission expression with AND/OR operators
/// Supports: AND, OR, and parentheses
/// Operator precedence: OR < AND < parentheses
pub fn evaluate_permission_expr(key_permissions: &HashSet<&str>, expr: &str) -> bool {
    let expr = expr.trim();

    // Handle parentheses first (highest precedence)
    if let Some((open_pos, close_pos)) = find_matching_paren(expr, '(') {
        // Evaluate inside parentheses first
        let inner = &expr[open_pos + 1..close_pos];
        let inner_result = evaluate_permission_expr(key_permissions, inner);

        // Replace the parenthesized expression with its result
        // We need to reconstruct: "before <result> after"
        let before = &expr[..open_pos];
        let after = &expr[close_pos + 1..];

        // Determine what operator connects before and after
        // If before ends with " OR ", it's OR, otherwise check for " AND "
        let new_expr = if before.trim().is_empty() {
            // No "before" part
            if after.trim().is_empty() {
                // Just the parenthesized expression
                return inner_result;
            } else if after.trim_start().starts_with(" OR ") {
                format!("{} OR {}", inner_result, &after.trim_start()[4..])
            } else if after.trim_start().starts_with(" AND ") {
                format!("{} AND {}", inner_result, &after.trim_start()[5..])
            } else {
                format!("{} {}", inner_result, after.trim())
            }
        } else {
            // Has "before" part - check what connects it
            if before.trim().ends_with(" OR ") {
                let before_clean = before.trim()[..before.trim().len() - 4].trim();
                if after.trim().is_empty() {
                    format!("{} OR {}", before_clean, inner_result)
                } else {
                    format!("{} OR {} {}", before_clean, inner_result, after.trim())
                }
            } else if before.trim().ends_with(" AND ") {
                let before_clean = before.trim()[..before.trim().len() - 5].trim();
                if after.trim().is_empty() {
                    format!("{} AND {}", before_clean, inner_result)
                } else {
                    format!("{} AND {} {}", before_clean, inner_result, after.trim())
                }
            } else {
                // No operator before, check after
                if after.trim_start().starts_with(" OR ") {
                    format!("{} OR {}", inner_result, &after.trim_start()[4..])
                } else if after.trim_start().starts_with(" AND ") {
                    format!("{} AND {}", inner_result, &after.trim_start()[5..])
                } else {
                    format!("{} {} {}", before.trim(), inner_result, after.trim())
                }
            }
        };

        return evaluate_permission_expr(key_permissions, &new_expr);
    }

    // Split by OR (lowest precedence)
    let or_parts: Vec<&str> = expr.split(" OR ").map(|s| s.trim()).collect();

    if or_parts.len() > 1 {
        return or_parts
            .iter()
            .any(|part| evaluate_and_expr(key_permissions, part));
    }

    evaluate_and_expr(key_permissions, expr)
}

/// Evaluate AND-connected permissions (highest precedence after parentheses)
pub fn evaluate_and_expr(key_permissions: &HashSet<&str>, expr: &str) -> bool {
    let and_parts: Vec<&str> = expr.split(" AND ").map(|s| s.trim()).collect();

    and_parts.iter().all(|part| {
        if part.contains('(') {
            evaluate_permission_expr(key_permissions, part)
        } else {
            key_permissions.contains(*part)
        }
    })
}

/// Find matching parenthesis and return (open_pos, close_pos)
pub fn find_matching_paren(expr: &str, open: char) -> Option<(usize, usize)> {
    let mut depth = 0;
    let mut open_pos = None;

    for (i, c) in expr.chars().enumerate() {
        if c == open {
            if depth == 0 {
                open_pos = Some(i);
            }
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth == 0 {
                if let Some(op) = open_pos {
                    return Some((op, i));
                }
            }
        }
    }
    None
}

/// Parse X-Wacht-Tag-* headers into a HashMap
/// Example: X-Wacht-Tag-region: us-east-1 -> {"region": "us-east-1"}
pub fn parse_tag_headers(headers: &HeaderMap) -> HashMap<String, String> {
    let mut tags = HashMap::new();

    for (name, value) in headers.iter() {
        let name_str = name.as_str();
        if let Some(tag_key) = name_str.strip_prefix("x-wacht-tag-") {
            if let Ok(value_str) = value.to_str() {
                tags.insert(tag_key.to_string(), value_str.to_string());
            }
        }
    }

    tags
}
