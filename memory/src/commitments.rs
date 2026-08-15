use contracts::Storage;

use crate::errors::MemoryError;

/// Курируемый LLM список открытых задач/обещаний — один общий файл на всю
/// личность, не по чату (как `working_memory.md` у kuni: обещание, данное в
/// одном разговоре, должно быть видно боту в любом другом). Обновляется тем
/// же вызовом, что и извлечение фактов (см. `extraction::maybe_extract`),
/// читается на каждый ход (`PersonalityMemory::commitments`).
#[derive(Clone)]
pub struct CommitmentsStore<S> {
    storage: S,
    key: String,
}

impl<S> CommitmentsStore<S>
where
    S: Storage + Clone + Send + Sync + 'static,
{
    pub fn new(storage: S, key: impl Into<String>) -> Self {
        Self {
            storage,
            key: key.into(),
        }
    }

    /// `None`, если ничего ещё не сохранено (или чтение не удалось по любой
    /// другой причине) — в отличие от пустой строки (список есть, но сейчас
    /// пуст).
    pub async fn get(&self) -> Option<String> {
        self.storage.get(self.key.clone()).await.ok()
    }

    pub async fn set(&self, text: String) -> Result<(), MemoryError> {
        self.storage.set(self.key.clone(), text).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::InMemoryStorage;

    fn store() -> CommitmentsStore<InMemoryStorage> {
        CommitmentsStore::new(InMemoryStorage::default(), "commitments.md")
    }

    #[tokio::test]
    async fn nothing_saved_returns_none() {
        let store = store();

        assert_eq!(store.get().await, None);
    }

    #[tokio::test]
    async fn set_then_get_round_trips() {
        let store = store();

        store
            .set("напомнить про дедлайн".to_owned())
            .await
            .expect("set succeeds");
        assert_eq!(store.get().await, Some("напомнить про дедлайн".to_owned()));
    }

    #[tokio::test]
    async fn set_overwrites_previous_value() {
        let store = store();

        store.set("первое".to_owned()).await.expect("set succeeds");
        store.set("второе".to_owned()).await.expect("set succeeds");
        assert_eq!(store.get().await, Some("второе".to_owned()));
    }
}
