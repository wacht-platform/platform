pub fn sanitize_filename(name: &str) -> Option<String> {
    let mut out = String::with_capacity(name.len());
    let mut prev_underscore = false;

    for ch in name.chars() {
        let is_allowed = ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-';
        if is_allowed {
            out.push(ch);
            prev_underscore = false;
        } else if !prev_underscore {
            out.push('_');
            prev_underscore = true;
        }
    }

    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
