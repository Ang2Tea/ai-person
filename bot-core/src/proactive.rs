use std::time::Duration;

use chrono::{DateTime, Utc};
use rand::RngExt;
use teloxide::types::ChatId;

use contracts::{Llm, Storage};

use crate::bot::ChatBot;
use crate::buffer::BufferStore;
use crate::settings::ProactiveSettings;

/// Периодически выбирает малоактивный известный чат и с настраиваемым шансом
/// даёт модели шанс написать в него первой (`ChatBot::run_proactive`) — сама
/// отправка (или отказ) остаётся решением модели, воркер только выбирает,
/// в какой чат постучаться и когда.
pub fn spawn_task<L, B>(chat_bot: ChatBot<L, B>, buffer: BufferStore<B>, settings: ProactiveSettings)
where
    L: Llm + Clone + Send + Sync + 'static,
    B: Storage + Clone + Send + Sync + 'static,
{
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(settings.interval_minutes * 60));
        ticker.tick().await; // первый tick — немедленно; пропускаем, чтобы не писать сразу при старте

        loop {
            ticker.tick().await;

            if !roll_probability(settings.probability) {
                tracing::debug!("proactive: skipped by probability roll");
                continue;
            }

            let Some(chat_id) = pick_eligible_chat(&buffer, settings.min_inactivity_minutes).await
            else {
                tracing::debug!("proactive: no eligible chat found");
                continue;
            };

            tracing::info!(chat_id, "proactive: giving the model a chance to write first");
            if let Err(err) = chat_bot.run_proactive(ChatId(chat_id)).await {
                tracing::error!(chat_id, %err, "proactive turn failed");
            }
        }
    });
}

fn roll_probability(p: f32) -> bool {
    rand::rng().random::<f32>() < p
}

async fn pick_eligible_chat<B>(buffer: &BufferStore<B>, min_inactivity_minutes: i64) -> Option<i64>
where
    B: Storage + Clone + Send + Sync + 'static,
{
    let mut activity = Vec::new();
    for chat_id in buffer.chat_ids().await {
        let last_activity = buffer.get(chat_id).await.and_then(|b| b.last_activity());
        activity.push((chat_id, last_activity));
    }

    let eligible = filter_eligible(&activity, Utc::now(), min_inactivity_minutes);
    if eligible.is_empty() {
        return None;
    }

    let idx = rand::rng().random_range(0..eligible.len());
    Some(eligible[idx])
}

/// Чистая (без I/O) фильтрация чатов по давности последней активности — для
/// проверки без сети/файлов.
fn filter_eligible(
    activity: &[(i64, Option<DateTime<Utc>>)],
    now: DateTime<Utc>,
    min_inactivity_minutes: i64,
) -> Vec<i64> {
    activity
        .iter()
        .filter_map(|(chat_id, last_activity)| {
            let last_activity = (*last_activity)?;
            (now - last_activity >= chrono::Duration::minutes(min_inactivity_minutes))
                .then_some(*chat_id)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_recently_active_and_unknown_chats() {
        let now = Utc::now();
        let activity = vec![
            (1, Some(now - chrono::Duration::minutes(120))), // давно неактивен — подходит
            (2, Some(now - chrono::Duration::minutes(10))),  // активен только что — не подходит
            (3, None),                                       // нет ни одного сообщения — не подходит
        ];

        let eligible = filter_eligible(&activity, now, 90);
        assert_eq!(eligible, vec![1]);
    }

    #[test]
    fn boundary_inactivity_is_eligible() {
        let now = Utc::now();
        let activity = vec![(1, Some(now - chrono::Duration::minutes(90)))];

        let eligible = filter_eligible(&activity, now, 90);
        assert_eq!(eligible, vec![1]);
    }
}
