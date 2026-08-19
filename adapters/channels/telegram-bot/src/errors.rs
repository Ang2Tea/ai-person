use thiserror::Error;

#[derive(Debug, Error)]
pub enum HistoryError {
    #[error(transparent)]
    Storage(#[from] contracts::StorageError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum DispatchError {
    #[error(transparent)]
    App(#[from] bot_core::errors::AppError),
    #[error(transparent)]
    Telegram(#[from] teloxide::RequestError),
}
