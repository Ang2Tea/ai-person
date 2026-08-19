use contracts::{Storage, StorageError};

use crate::errors::MemoryError;
use crate::record::MemoryRecord;

#[derive(Clone)]
pub struct MemoryStore<S> {
    storage: S,
}

impl<S> MemoryStore<S>
where
    S: Storage + Clone + Send + Sync + 'static,
{
    pub fn new(storage: S) -> Self {
        Self { storage }
    }

    /// Читает и парсит только те записи, чьё имя (уже полученное дешёвым
    /// листингом, без чтения содержимого) проходит `predicate` — само имя
    /// кодирует `origin_chat_id`/`visibility`/`about_users` (см.
    /// `MemoryRecord::filename`).
    pub async fn list_filtered(
        &self,
        predicate: impl Fn(&str) -> bool + Send + 'static,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        let names = self.storage.list(String::new()).await?;

        let mut records = Vec::new();
        for name in names {
            if !name.ends_with(".md") || !predicate(&name) {
                continue;
            }
            let raw = self.storage.get(name).await?;
            records.push(MemoryRecord::from_markdown(&raw)?);
        }
        Ok(records)
    }

    pub async fn list_all(&self) -> Result<Vec<MemoryRecord>, MemoryError> {
        self.list_filtered(|_| true).await
    }

    pub async fn append(&self, record: &MemoryRecord) -> Result<(), MemoryError> {
        self.storage
            .set(record.filename(), record.to_markdown()?)
            .await?;
        Ok(())
    }

    pub async fn touch(&self, record: &MemoryRecord) -> Result<(), MemoryError> {
        // Имя файла зависит только от origin_chat_id/visibility/about_users/id —
        // они не меняются после создания, так что это всегда тот же ключ.
        self.append(record).await
    }

    pub async fn remove(&self, record: &MemoryRecord) -> Result<(), MemoryError> {
        match self.storage.delete(record.filename()).await {
            Ok(()) => Ok(()),
            Err(StorageError::NotFound(_)) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::Visibility;
    use crate::test_support::InMemoryStorage;

    fn store() -> MemoryStore<InMemoryStorage> {
        MemoryStore::new(InMemoryStorage::default())
    }

    #[tokio::test]
    async fn remove_deletes_record_file() {
        let store = store();

        let record = MemoryRecord::new("факт для удаления", 0.0, Visibility::Private, vec![], 1, vec![1.0]);
        store.append(&record).await.expect("append succeeds");
        assert_eq!(store.list_all().await.expect("list succeeds").len(), 1);

        store.remove(&record).await.expect("remove succeeds");
        assert!(store.list_all().await.expect("list succeeds").is_empty());

        // Повторное удаление уже отсутствующего файла не должно быть ошибкой.
        store.remove(&record).await.expect("remove is idempotent");
    }
}
