use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{buffer::ChatBuffer, errors::BufferError};

pub trait BufferStorage: Send + Sync {
    fn load(&self) -> Result<HashMap<i64, ChatBuffer>, BufferError>;
    fn save(&self, buffers: &HashMap<i64, ChatBuffer>) -> Result<(), BufferError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}
