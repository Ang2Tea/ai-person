use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use crate::errors::MemoryError;
use crate::memory::record::MemoryRecord;

pub struct LocalMemoryStorage {
    root: PathBuf,
}

impl LocalMemoryStorage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Читает и парсит только те файлы, чьё имя проходит `predicate` — само имя
    /// кодирует `origin_chat_id`/`visibility`/`about_users` (см.
    /// `MemoryRecord::filename`), так что нерелевантные записи отсеиваются
    /// дешёвым листингом директории, не открывая и не парся их содержимое
    /// (включая вектор эмбеддинга — самую дорогую часть файла).
    pub async fn list_filtered(
        &self,
        predicate: impl Fn(&str) -> bool + Send + 'static,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            if !root.exists() {
                return Ok(Vec::new());
            }

            let mut records = Vec::new();
            for entry in fs::read_dir(&root)? {
                let path = entry?.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if !predicate(name) {
                    continue;
                }
                let raw = fs::read_to_string(&path)?;
                records.push(MemoryRecord::from_markdown(&raw)?);
            }
            Ok(records)
        })
        .await
        .expect("blocking task panicked")
    }

    pub async fn list_all(&self) -> Result<Vec<MemoryRecord>, MemoryError> {
        self.list_filtered(|_| true).await
    }

    pub async fn append(&self, record: &MemoryRecord) -> Result<(), MemoryError> {
        let root = self.root.clone();
        let filename = record.filename();
        let content = record.to_markdown();

        tokio::task::spawn_blocking(move || {
            fs::create_dir_all(&root)?;
            let final_path = root.join(&filename);
            // Временный файл + rename — атомарно на одной файловой системе,
            // защищает от битого факт-файла при падении процесса посреди записи.
            let tmp_path = root.join(format!("{filename}.tmp"));
            fs::write(&tmp_path, content)?;
            fs::rename(&tmp_path, &final_path)?;
            Ok(())
        })
        .await
        .expect("blocking task panicked")
    }

    pub async fn touch(&self, record: &MemoryRecord) -> Result<(), MemoryError> {
        // Имя файла зависит только от origin_chat_id/visibility/about_users/id —
        // они не меняются после создания, так что это всегда тот же файл.
        self.append(record).await
    }
}

#[derive(Clone)]
pub struct MemoryStore(Arc<LocalMemoryStorage>);

impl MemoryStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self(Arc::new(LocalMemoryStorage::new(root)))
    }

    pub async fn list_filtered(
        &self,
        predicate: impl Fn(&str) -> bool + Send + 'static,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        self.0.list_filtered(predicate).await
    }

    pub async fn list_all(&self) -> Result<Vec<MemoryRecord>, MemoryError> {
        self.0.list_all().await
    }

    pub async fn append(&self, record: &MemoryRecord) -> Result<(), MemoryError> {
        self.0.append(record).await
    }

    pub async fn touch(&self, record: &MemoryRecord) -> Result<(), MemoryError> {
        self.0.touch(record).await
    }
}
