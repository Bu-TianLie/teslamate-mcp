use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Not found: {0}")]
    NotFound(String),
}

impl From<AppError> for rmcp::Error {
    fn from(err: AppError) -> Self {
        rmcp::Error::internal_error(err.to_string(), None)
    }
}
