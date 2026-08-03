use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Llm(#[from] LlmError),
    #[error(transparent)]
    Telegram(#[from] teloxide::RequestError),
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("model returned no choices")]
    EmptyResponse,
}

#[derive(Debug, Error)]
pub enum BufferError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("tool failed: {0}")]
    Failed(String),
}

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid record format: {0}")]
    Format(String),
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error(transparent)]
    Llm(#[from] LlmError),
}

#[derive(Debug, Error)]
pub enum ConsolidationError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Memory(#[from] MemoryError),
    #[error(transparent)]
    Llm(#[from] LlmError),
}
