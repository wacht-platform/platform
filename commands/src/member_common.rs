use common::error::AppError;

pub(crate) fn username_or_none(username: String) -> Option<String> {
    if username.is_empty() {
        None
    } else {
        Some(username)
    }
}

pub(crate) fn ensure_membership_exists(
    exists: bool,
    entity_name: &'static str,
) -> Result<(), AppError> {
    if exists {
        Ok(())
    } else {
        Err(AppError::NotFound(format!("{entity_name} not found")))
    }
}
