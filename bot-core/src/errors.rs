use contracts::LlmError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Llm(#[from] LlmError),
}
