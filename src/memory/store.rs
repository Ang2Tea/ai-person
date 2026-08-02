use std::collections::HashSet;
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

    pub fn list(&self, folder: &str) -> Result<Vec<MemoryRecord>, MemoryError> {
        let dir = self.root.join(folder);
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut records = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let raw = fs::read_to_string(&path)?;
            records.push(MemoryRecord::from_markdown(&raw)?);
        }
        Ok(records)
    }

    pub fn append(&self, folder: &str, record: &MemoryRecord) -> Result<(), MemoryError> {
        let dir = self.root.join(folder);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join(format!("{}.md", record.id)), record.to_markdown())?;
        Ok(())
    }

    pub fn touch(&self, folder: &str, record: &MemoryRecord) -> Result<(), MemoryError> {
        // Same file (id doesn't change), just rewritten with updated lastUsed/usageCount.
        self.append(folder, record)
    }
}

#[derive(Clone)]
pub struct MemoryStore(Arc<LocalMemoryStorage>);

impl MemoryStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self(Arc::new(LocalMemoryStorage::new(root)))
    }

    pub fn list(&self, folder: &str) -> Result<Vec<MemoryRecord>, MemoryError> {
        self.0.list(folder)
    }

    pub fn append(&self, folder: &str, record: &MemoryRecord) -> Result<(), MemoryError> {
        self.0.append(folder, record)
    }

    pub fn touch(&self, folder: &str, record: &MemoryRecord) -> Result<(), MemoryError> {
        self.0.touch(folder, record)
    }

    /// Папки-кандидаты для поиска в контексте `chat_id`: всегда своя папка, а для
    /// группового чата (`chat_id < 0`) — ещё и папки пользователей, уже упомянутых
    /// в `about_users` собственных записей группы (Telegram Bot API не даёт списка
    /// участников группы, поэтому реальный список участников так не получить).
    pub fn candidate_folders(&self, chat_id: i64) -> Vec<String> {
        let mut folders = vec![chat_id.to_string()];

        if chat_id < 0 {
            let mut seen = HashSet::new();
            if let Ok(records) = self.list(&chat_id.to_string()) {
                for user_id in records.iter().flat_map(|r| &r.about_users) {
                    if seen.insert(*user_id) {
                        folders.push(user_id.to_string());
                    }
                }
            }
        }

        folders
    }
}
