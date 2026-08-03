use std::{collections::HashMap, fs, path::PathBuf};

use crate::{buffer::ChatBuffer, contracts::BufferStorage, errors::BufferError};

#[derive(Clone)]
pub struct LocalFileStorage {
    path: PathBuf,
}

impl LocalFileStorage {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl BufferStorage for LocalFileStorage {
    async fn load(&self) -> Result<HashMap<i64, ChatBuffer>, BufferError> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            if !path.exists() {
                return Ok(HashMap::new());
            }
            let raw = fs::read_to_string(&path)?;
            Ok(serde_json::from_str(&raw)?)
        })
        .await
        .expect("blocking task panicked")
    }

    async fn save(&self, buffers: &HashMap<i64, ChatBuffer>) -> Result<(), BufferError> {
        let path = self.path.clone();
        let json = serde_json::to_string_pretty(buffers)?;

        tokio::task::spawn_blocking(move || {
            // Пишем во временный файл рядом и переименовываем поверх целевого —
            // переименование в пределах одной файловой системы атомарно, так что
            // падение процесса посреди записи не оставит битый working_memory.json.
            let tmp_path = PathBuf::from(format!("{}.tmp", path.display()));
            fs::write(&tmp_path, json)?;
            fs::rename(&tmp_path, &path)?;
            Ok(())
        })
        .await
        .expect("blocking task panicked")
    }
}
