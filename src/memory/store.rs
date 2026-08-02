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

    pub fn list_all(&self) -> Result<Vec<MemoryRecord>, MemoryError> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }

        let mut records = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let raw = fs::read_to_string(&path)?;
            records.push(MemoryRecord::from_markdown(&raw)?);
        }
        Ok(records)
    }

    pub fn append(&self, record: &MemoryRecord) -> Result<(), MemoryError> {
        fs::create_dir_all(&self.root)?;
        fs::write(self.root.join(format!("{}.md", record.id)), record.to_markdown())?;
        Ok(())
    }

    pub fn touch(&self, record: &MemoryRecord) -> Result<(), MemoryError> {
        // Same file (id doesn't change), just rewritten with updated lastUsed/usageCount.
        self.append(record)
    }
}

#[derive(Clone)]
pub struct MemoryStore(Arc<LocalMemoryStorage>);

impl MemoryStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self(Arc::new(LocalMemoryStorage::new(root)))
    }

    pub fn list_all(&self) -> Result<Vec<MemoryRecord>, MemoryError> {
        self.0.list_all()
    }

    pub fn append(&self, record: &MemoryRecord) -> Result<(), MemoryError> {
        self.0.append(record)
    }

    pub fn touch(&self, record: &MemoryRecord) -> Result<(), MemoryError> {
        self.0.touch(record)
    }
}
