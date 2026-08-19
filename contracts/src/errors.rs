use thiserror::Error;

/// Ошибка `Storage`, не привязанная к конкретной реализации — каждый бэкенд
/// сам переводит свою ошибку в текст, чтобы `contracts` не знал ни про один
/// из них.
#[derive(Debug, Error)]
pub enum StorageError {
    /// `get`/`set`/`list`/`delete` не смогли выполниться по причине, не
    /// связанной с отсутствием ключа (недоступен диск/сеть, отказано в
    /// доступе и т.п.) — вызывающий код такую ошибку пробрасывает дальше, а
    /// не трактует как "данных нет".
    #[error("storage backend error: {0}")]
    Backend(String),

    /// Ключа с таким именем не существует — ожидаемый исход `get`/`delete`
    /// (например, ещё ни разу не записанные commitments нового чата), а не
    /// сбой.
    #[error("key not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("request failed: {0}")]
    Request(String),

    #[error("model returned no choices")]
    EmptyResponse,
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("tool failed: {0}")]
    Failed(String),
}
