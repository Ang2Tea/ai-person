use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, OwnedMutexGuard};

/// Сериализует обработку сообщений одного и того же чата — без этого
/// `teloxide::repl` обрабатывает апдейты конкурентно, и два быстрых сообщения
/// подряд могут независимо уйти в LLM и вернуться в произвольном порядке.
/// Разные чаты обрабатываются параллельно, лок глобальным не является.
#[derive(Clone, Default)]
pub struct ChatLocks(Arc<Mutex<HashMap<i64, Arc<Mutex<()>>>>>);

impl ChatLocks {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn lock(&self, chat_id: i64) -> OwnedMutexGuard<()> {
        let mutex = {
            let mut map = self.0.lock().await;
            map.entry(chat_id)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        mutex.lock_owned().await
    }
}
