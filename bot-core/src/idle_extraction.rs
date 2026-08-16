use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tracing::Instrument;

use contracts::{Activity, BackgroundJob, ChannelHistory, ChannelId, Memory};

/// Как часто сканировать известные чаты — деталь реализации, не настройка
/// (сам скан дешёвый, читает только уже загруженную в память историю), по
/// аналогии с `FLUSH_INTERVAL` бывшего `BufferStore`.
const CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Второй, независимый от порога токенов повод извлечь факты из чата в
/// долгосрочную память — простой. Если в чате давно не было сообщений, а
/// накопилось больше сообщений, чем `Memory::keep_last_messages` (то, что
/// извлечение оставляет в истории после себя) — смысл разговора уже
/// случился, ждать накопления токенов незачем.
pub struct IdleExtractionJob<M> {
    history: Arc<dyn ChannelHistory>,
    memory: M,
    idle_extraction_after_minutes: i64,
}

impl<M> IdleExtractionJob<M>
where
    M: Memory + Clone + Send + Sync + 'static,
{
    pub fn new(
        history: Arc<dyn ChannelHistory>,
        memory: M,
        idle_extraction_after_minutes: i64,
    ) -> Self {
        Self {
            history,
            memory,
            idle_extraction_after_minutes,
        }
    }
}

impl<M> BackgroundJob for IdleExtractionJob<M>
where
    M: Memory + Clone + Send + Sync + 'static,
{
    fn spawn(self: Box<Self>) {
        let IdleExtractionJob {
            history,
            memory,
            idle_extraction_after_minutes,
        } = *self;

        crate::scheduler::spawn_periodic(CHECK_INTERVAL, move || {
            let history = history.clone();
            let memory = memory.clone();

            async move {
                for chat in history.known_chats().await {
                    let activity = history.activity(&chat).await;
                    if !is_ready(
                        activity,
                        Utc::now(),
                        idle_extraction_after_minutes,
                        memory.keep_last_messages(),
                    ) {
                        continue;
                    }

                    let chat_id = chat.id.clone();
                    async {
                        tracing::info!("idle extraction: chat quiet for a while, extracting");
                        let transcript = history.transcript(&chat).await;
                        memory.extract(channel_chat_id(&chat), &transcript).await;
                        history
                            .truncate_keep_last(&chat, memory.keep_last_messages())
                            .await;
                    }
                    .instrument(tracing::info_span!("idle_extraction_job", chat_id))
                    .await;
                }
            }
        });
    }
}

fn channel_chat_id(id: &ChannelId) -> i64 {
    id.id.parse().unwrap_or(0)
}

fn is_ready(
    activity: Option<Activity>,
    now: DateTime<Utc>,
    idle_after_minutes: i64,
    keep_last_messages: usize,
) -> bool {
    let Some((last_activity, message_count)) = activity else {
        return false;
    };

    message_count > keep_last_messages
        && now - last_activity >= chrono::Duration::minutes(idle_after_minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_chat_without_messages_is_not_ready() {
        assert!(!is_ready(None, Utc::now(), 30, 5));
    }

    #[test]
    fn small_talk_within_keep_last_is_not_ready_even_if_idle() {
        let now = Utc::now();
        let activity = Some((now - chrono::Duration::minutes(120), 3));
        assert!(!is_ready(activity, now, 30, 5));
    }

    #[test]
    fn recent_activity_is_not_ready_even_with_enough_messages() {
        let now = Utc::now();
        let activity = Some((now - chrono::Duration::minutes(5), 10));
        assert!(!is_ready(activity, now, 30, 5));
    }

    #[test]
    fn idle_chat_with_extra_messages_is_ready() {
        let now = Utc::now();
        let activity = Some((now - chrono::Duration::minutes(31), 10));
        assert!(is_ready(activity, now, 30, 5));
    }
}
