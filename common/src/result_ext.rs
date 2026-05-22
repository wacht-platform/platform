use models::error::AppError;
use std::fmt::Display;

pub trait ResultExt<T> {
    fn map_err_internal<S: Into<String>>(self, ctx: S) -> Result<T, AppError>;
}

impl<T, E: Display> ResultExt<T> for Result<T, E> {
    fn map_err_internal<S: Into<String>>(self, ctx: S) -> Result<T, AppError> {
        self.map_err(|e| AppError::Internal(format!("{}: {}", ctx.into(), e)))
    }
}
