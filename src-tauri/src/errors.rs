use serde::Serialize;
use std::fmt;

/// Structured error sent to the UI (no raw stack traces).
#[derive(Debug, Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub operation: String,
}

impl AppError {
    pub fn new(code: &str, message: impl Into<String>, operation: &str) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            operation: operation.to_string(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {} ({})", self.code, self.message, self.operation)
    }
}

impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        let msg = match &e {
            rusqlite::Error::SqliteFailure(err, _)
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                "A database constraint was violated (duplicate name or invalid value)".to_string()
            }
            _ => {
                let mut m = "Database operation failed".to_string();
                // Detail is only included in debug builds, never in release.
                if cfg!(debug_assertions) {
                    m.push_str(&format!(": {e}"));
                }
                m
            }
        };
        AppError::new("DB_ERROR", msg, "database")
    }
}