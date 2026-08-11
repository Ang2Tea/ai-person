use contracts::Storage;

use crate::errors::MemoryError;

/// Курируемый LLM список открытых задач/обещаний, отдельно по чату — отдельный
/// ключ на чат (`{chat_id}.md`), не часть буфера переписки и не факт
/// дневника. Обновляется тем же вызовом, что и извлечение фактов (см.
/// `extraction::maybe_extract`), читается на каждый ход (`PersonalityMemory::commitments`).
#[derive(Clone)]
pub struct CommitmentsStore<S> {
    storage: S,
}

impl<S> CommitmentsStore<S>
where
    S: Storage + Clone + Send + Sync + 'static,
{
    pub fn new(storage: S) -> Self {
        Self { storage }
    }

    fn key(chat_id: i64) -> String {
        format!("{chat_id}.md")
    }

    /// `None`, если для этого чата ещё ничего не сохранено (или чтение не
    /// удалось по любой другой причине) — в отличие от пустой строки (список
    /// есть, но сейчас пуст).
    pub async fn get(&self, chat_id: i64) -> Option<String> {
        self.storage.get(Self::key(chat_id)).await.ok()
    }

    pub async fn set(&self, chat_id: i64, text: String) -> Result<(), MemoryError> {
        self.storage.set(Self::key(chat_id), text).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::InMemoryStorage;

    fn store() -> CommitmentsStore<InMemoryStorage> {
        CommitmentsStore::new(InMemoryStorage::default())
    }

    #[tokio::test]
    async fn unknown_chat_returns_none() {
        let store = store();

        assert_eq!(store.get(1).await, None);
    }

    #[tokio::test]
    async fn set_then_get_round_trips() {
        let store = store();

        store
            .set(1, "напомнить про дедлайн".to_owned())
            .await
            .expect("set succeeds");
        assert_eq!(store.get(1).await, Some("напомнить про дедлайн".to_owned()));

        // Другой чат не видит чужой список.
        assert_eq!(store.get(2).await, None);
    }

    #[tokio::test]
    async fn set_overwrites_previous_value() {
        let store = store();

        store.set(1, "первое".to_owned()).await.expect("set succeeds");
        store.set(1, "второе".to_owned()).await.expect("set succeeds");
        assert_eq!(store.get(1).await, Some("второе".to_owned()));
    }
}
