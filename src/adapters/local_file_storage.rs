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
    fn load(&self) -> Result<HashMap<i64, ChatBuffer>, BufferError> {
        if !self.path.exists() {
            return Ok(HashMap::new());
        }
        let raw = fs::read_to_string(&self.path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    fn save(&self, buffers: &HashMap<i64, ChatBuffer>) -> Result<(), BufferError> {
        let json = serde_json::to_string_pretty(buffers)?;
        fs::write(&self.path, json)?;
        Ok(())
    }
}
