use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("workbook error: {0}")]
    Workbook(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("path is outside its configured root: {0}")]
    UnsafePath(PathBuf),
    #[error("resource not found: {0}")]
    NotFound(String),
    #[error("operation cancelled")]
    Cancelled,
}

pub type CoreResult<T> = Result<T, CoreError>;
