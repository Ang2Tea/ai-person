use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use crate::errors::MemoryError;

struct LocalCommitmentsStorage {
    root: PathBuf,
}

impl LocalCommitmentsStorage {
    fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path(&self, chat_id: i64) -> PathBuf {
        self.root.join(format!("{chat_id}.md"))
    }

    async fn get(&self, chat_id: i64) -> Option<String> {
        let path = self.path(chat_id);

        tokio::task::spawn_blocking(move || fs::read_to_string(&path).ok())
            .await
            .expect("blocking task panicked")
    }

    async fn set(&self, chat_id: i64, text: String) -> Result<(), MemoryError> {
        let root = self.root.clone();
        let path = self.path(chat_id);
        let tmp_path = root.join(format!("{chat_id}.md.tmp"));

        tokio::task::spawn_blocking(move || {
            fs::create_dir_all(&root)?;
            fs::write(&tmp_path, text)?;
            fs::rename(&tmp_path, &path)?;
            Ok(())
        })
        .await
        .expect("blocking task panicked")
    }
}

/// Курируемый LLM список открытых задач/обещаний, отдельно по чату — отдельный
/// md-файл на чат (`{chat_id}.md`), не часть буфера переписки и не факт
/// дневника. Обновляется тем же вызовом, что и извлечение фактов (см.
/// `memory::maybe_extract`), читается на каждый ход (`ChatBot::build_messages`).
#[derive(Clone)]
pub struct CommitmentsStore(Arc<LocalCommitmentsStorage>);

impl CommitmentsStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self(Arc::new(LocalCommitmentsStorage::new(root)))
    }

    /// `None`, если для этого чата ещё ничего не сохранено — в отличие от
    /// пустой строки (список есть, но сейчас пуст).
    pub async fn get(&self, chat_id: i64) -> Option<String> {
        self.0.get(chat_id).await
    }

    pub async fn set(&self, chat_id: i64, text: String) -> Result<(), MemoryError> {
        self.0.set(chat_id, text).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "ai-chat-person-commitments-test-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ))
    }

    #[tokio::test]
    async fn unknown_chat_returns_none() {
        let dir = temp_dir();
        let store = CommitmentsStore::new(&dir);

        assert_eq!(store.get(1).await, None);

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn set_then_get_round_trips() {
        let dir = temp_dir();
        let store = CommitmentsStore::new(&dir);

        store
            .set(1, "напомнить про дедлайн".to_owned())
            .await
            .expect("set succeeds");
        assert_eq!(store.get(1).await, Some("напомнить про дедлайн".to_owned()));

        // Другой чат не видит чужой список.
        assert_eq!(store.get(2).await, None);

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn set_overwrites_previous_value() {
        let dir = temp_dir();
        let store = CommitmentsStore::new(&dir);

        store.set(1, "первое".to_owned()).await.expect("set succeeds");
        store.set(1, "второе".to_owned()).await.expect("set succeeds");
        assert_eq!(store.get(1).await, Some("второе".to_owned()));

        let _ = fs::remove_dir_all(&dir);
    }
}
