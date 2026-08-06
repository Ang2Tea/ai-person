use contracts::LlmError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Llm(#[from] LlmError),
    #[error(transparent)]
    Telegram(#[from] teloxide::RequestError),
}

#[derive(Debug, Error)]
pub enum BufferError {
    #[error(transparent)]
    Storage(#[from] contracts::StorageError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error(transparent)]
    Storage(#[from] contracts::StorageError),
    #[error("invalid record format: {0}")]
    Format(String),
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error(transparent)]
    Llm(#[from] LlmError),
}

#[derive(Debug, Error)]
pub enum ConsolidationError {
    #[error(transparent)]
    Storage(#[from] contracts::StorageError),
    #[error(transparent)]
    Memory(#[from] MemoryError),
    #[error(transparent)]
    Llm(#[from] LlmError),
}
