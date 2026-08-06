use chrono::{DateTime, Utc};
use contracts::{ChatMessage, Storage, StorageError};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::RwLock;

use crate::errors::BufferError;

/// Ключ, под которым весь буфер (все чаты разом) лежит в `Storage` — буфер
/// всегда читается/пишется целиком, отдельного ключа на чат не заводим.
const BUFFER_KEY: &str = "working_memory.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferedMessage {
    pub telegram_message_id: i32,
    pub sender_id: i64,
    pub sender_name: String,
    pub text: String,
    pub timestamp: DateTime<Utc>,
    pub is_bot: bool, // true для собственных ответов бота
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatBuffer {
    messages: VecDeque<BufferedMessage>,
}

impl ChatBuffer {
    pub fn push(&mut self, msg: BufferedMessage) {
        self.messages.push_back(msg);
    }

    pub fn to_transcript(&self) -> String {
        self.messages
            .iter()
            .map(|m| {
                let who = if m.is_bot { "Бот" } else { &m.sender_name };
                format!(
                    "[{} #{}] {}: {}",
                    m.timestamp.format("%H:%M"),
                    m.telegram_message_id,
                    who,
                    m.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn to_request_messages(&self, system_prompt: &str) -> Vec<ChatMessage> {
        vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(self.to_transcript()),
        ]
    }

    pub fn truncate_keep_last(&mut self, n: usize) {
        while self.messages.len() > n {
            self.messages.pop_front();
        }
    }

    /// Имя последнего собеседника (не бота) — грубая метка для отображения чата
    /// человеку/модели, у нас нет отдельно хранимого названия чата/группы.
    pub fn last_sender_name(&self) -> Option<&str> {
        self.messages
            .iter()
            .rev()
            .find(|m| !m.is_bot)
            .map(|m| m.sender_name.as_str())
    }

    /// Время последнего события в чате (включая собственные ответы бота) — по
    /// нему проактивный воркер определяет, не идёт ли сейчас живой разговор.
    pub fn last_activity(&self) -> Option<DateTime<Utc>> {
        self.messages.back().map(|m| m.timestamp)
    }

    /// Число сообщений в буфере — по нему воркер извлечения по простою решает,
    /// есть ли вообще что извлекать сверх `keep_last_messages`.
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }
}

const FLUSH_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct BufferStore<S> {
    storage: S,
    buffers: Arc<RwLock<HashMap<i64, ChatBuffer>>>,
    dirty: Arc<AtomicBool>,
}

impl<S> BufferStore<S>
where
    S: Storage + Clone + Send + Sync + 'static,
{
    pub async fn new(storage: S) -> Result<Self, BufferError> {
        let buffers = match storage.get(BUFFER_KEY.to_owned()).await {
            Ok(raw) => serde_json::from_str(&raw)?,
            Err(StorageError::NotFound(_)) => HashMap::new(),
            Err(err) => return Err(BufferError::Storage(err)),
        };
        let store = Self {
            storage,
            buffers: Arc::new(RwLock::new(buffers)),
            dirty: Arc::new(AtomicBool::new(false)),
        };
        store.spawn_flush_task();
        Ok(store)
    }

    fn spawn_flush_task(&self) {
        let store = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(FLUSH_INTERVAL);
            loop {
                ticker.tick().await;
                if let Err(err) = store.flush().await {
                    tracing::error!(%err, "Can`t flush chat buffer to storage");
                }
            }
        });
    }

    pub async fn push(&self, chat_id: i64, msg: BufferedMessage) {
        let mut buffers = self.buffers.write().await;
        buffers.entry(chat_id).or_default().push(msg);
        self.dirty.store(true, Ordering::Release);
    }

    pub async fn get(&self, chat_id: i64) -> Option<ChatBuffer> {
        let buffers = self.buffers.read().await;
        buffers.get(&chat_id).cloned()
    }

    /// Все чаты, с которыми бот уже когда-либо взаимодействовал (ключи буфера).
    pub async fn chat_ids(&self) -> Vec<i64> {
        let buffers = self.buffers.read().await;
        buffers.keys().copied().collect()
    }

    pub async fn truncate_keep_last(&self, chat_id: i64, n: usize) {
        let mut buffers = self.buffers.write().await;
        if let Some(buffer) = buffers.get_mut(&chat_id) {
            buffer.truncate_keep_last(n);
        }
        self.dirty.store(true, Ordering::Release);
    }

    pub async fn flush(&self) -> Result<(), BufferError> {
        if !self.dirty.load(Ordering::Acquire) {
            return Ok(());
        }
        let json = {
            let buffers = self.buffers.read().await;
            serde_json::to_string_pretty(&*buffers)?
        };
        self.storage
            .set(BUFFER_KEY.to_owned(), json)
            .await
            .map_err(BufferError::Storage)?;
        self.dirty.store(false, Ordering::Release);
        Ok(())
    }
}
