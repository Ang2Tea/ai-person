use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::adapters::timeweb_client::TimewebClient;
use crate::buffer::{BufferStore, ChatBuffer};
use crate::commitments::CommitmentsStore;
use crate::contracts::BufferStorage;
use crate::memory::{self, MemoryStore};
use crate::settings::MemorySettings;

/// Как часто сканировать известные чаты — это деталь реализации, не настройка
/// (сам скан дешёвый, читает только уже загруженный в память буфер), по
/// аналогии с `FLUSH_INTERVAL` в `buffer.rs`.
const CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Второй, независимый от порога токенов повод извлечь факты из чата в
/// долгосрочную память — простой. Если в чате давно не было сообщений, а
/// накопилось больше, чем `keep_last_messages` (то, что `maybe_extract`
/// оставляет в буфере после себя) — смысл разговора уже случился, ждать
/// накопления токенов незачем.
#[allow(clippy::too_many_arguments)]
pub fn spawn_task<B>(
    llm: TimewebClient,
    memory: MemoryStore,
    buffer: BufferStore<B>,
    commitments: CommitmentsStore,
    settings: MemorySettings,
    model: Arc<str>,
    embedding_model: Arc<str>,
) where
    B: BufferStorage + Clone + Send + Sync + 'static,
{
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(CHECK_INTERVAL);
        loop {
            ticker.tick().await;

            for chat_id in buffer.chat_ids().await {
                let Some(chat_buffer) = buffer.get(chat_id).await else {
                    continue;
                };

                if !is_ready(
                    &chat_buffer,
                    Utc::now(),
                    settings.idle_extraction_after_minutes,
                    settings.keep_last_messages,
                ) {
                    continue;
                }

                tracing::info!(chat_id, "idle extraction: chat quiet for a while, extracting");
                if let Err(err) = memory::maybe_extract(
                    &llm,
                    &memory,
                    &buffer,
                    &commitments,
                    chat_id,
                    &settings,
                    &model,
                    &embedding_model,
                )
                .await
                {
                    tracing::error!(chat_id, %err, "idle extraction failed");
                }
            }
        }
    });
}

fn is_ready(
    chat_buffer: &ChatBuffer,
    now: DateTime<Utc>,
    idle_after_minutes: i64,
    keep_last_messages: usize,
) -> bool {
    let Some(last_activity) = chat_buffer.last_activity() else {
        return false;
    };

    chat_buffer.message_count() > keep_last_messages
        && now - last_activity >= chrono::Duration::minutes(idle_after_minutes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::BufferedMessage;

    fn buffer_with(count: usize, last_activity: DateTime<Utc>) -> ChatBuffer {
        let mut buffer = ChatBuffer::default();
        for i in 0..count {
            buffer.push(BufferedMessage {
                telegram_message_id: i as i32,
                sender_id: 1,
                sender_name: "Игорь".to_owned(),
                text: "привет".to_owned(),
                timestamp: last_activity,
                is_bot: false,
            });
        }
        buffer
    }

    #[test]
    fn fresh_chat_without_messages_is_not_ready() {
        let buffer = ChatBuffer::default();
        assert!(!is_ready(&buffer, Utc::now(), 30, 5));
    }

    #[test]
    fn small_talk_within_keep_last_is_not_ready_even_if_idle() {
        let now = Utc::now();
        let buffer = buffer_with(3, now - chrono::Duration::minutes(120));
        assert!(!is_ready(&buffer, now, 30, 5));
    }

    #[test]
    fn recent_activity_is_not_ready_even_with_enough_messages() {
        let now = Utc::now();
        let buffer = buffer_with(10, now - chrono::Duration::minutes(5));
        assert!(!is_ready(&buffer, now, 30, 5));
    }

    #[test]
    fn idle_chat_with_extra_messages_is_ready() {
        let now = Utc::now();
        let buffer = buffer_with(10, now - chrono::Duration::minutes(31));
        assert!(is_ready(&buffer, now, 30, 5));
    }
}
