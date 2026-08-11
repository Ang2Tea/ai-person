use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use contracts::{Storage, StorageError};

/// In-memory реализация `Storage` для unit-тестов — не оставляет файлов на
/// диске после себя.
#[derive(Clone, Default)]
pub(crate) struct InMemoryStorage(Arc<Mutex<HashMap<String, String>>>);

impl Storage for InMemoryStorage {
    async fn get(&self, key: String) -> Result<String, StorageError> {
        self.0
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&key)
            .cloned()
            .ok_or(StorageError::NotFound(key))
    }

    async fn list(&self, key: String) -> Result<Vec<String>, StorageError> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .keys()
            .filter(|k| k.starts_with(&key))
            .cloned()
            .collect())
    }

    async fn set(&self, key: String, content: String) -> Result<(), StorageError> {
        self.0
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(key, content);
        Ok(())
    }

    async fn delete(&self, key: String) -> Result<(), StorageError> {
        self.0
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(&key)
            .map(|_| ())
            .ok_or(StorageError::NotFound(key))
    }
}
