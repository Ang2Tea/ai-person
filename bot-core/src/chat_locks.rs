use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, OwnedMutexGuard};

use contracts::ChannelId;

/// Сериализует обработку сообщений одного и того же чата — без этого
/// конкурентная обработка входящих апдейтов могла бы независимо уйти в LLM и
/// вернуться в произвольном порядке. Разные чаты обрабатываются параллельно,
/// лок глобальным не является.
#[derive(Clone, Default)]
pub struct ChatLocks(Arc<Mutex<HashMap<ChannelId, Arc<Mutex<()>>>>>);

impl ChatLocks {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn lock(&self, chat: &ChannelId) -> OwnedMutexGuard<()> {
        let mutex = {
            let mut map = self.0.lock().await;
            map.entry(chat.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        mutex.lock_owned().await
    }
}
