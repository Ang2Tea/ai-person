use std::time::Duration;

use chrono::{DateTime, Utc};
use rand::RngExt;
use teloxide::{Bot, types::ChatId};
use tracing::Instrument;

use bot_core::bot::ChatBot;
use contracts::{BackgroundJob, ChannelId, Llm, Memory, Storage};

use crate::dispatch;
use crate::history::BufferStore;
use crate::settings::ProactiveSettings;

const TELEGRAM_CHANNEL: &str = "telegram";

/// Периодически выбирает малоактивный известный чат и с настраиваемым шансом
/// даёт модели шанс написать в него первой (`ChatBot::run_proactive`) — сама
/// отправка (или отказ) остаётся решением модели, воркер только выбирает,
/// в какой чат постучаться и когда. Нуждается в `teloxide::Bot`, чтобы
/// реально отправить сообщение — поэтому живёт здесь, не в `bot-core`.
pub struct ProactiveJob<L, M, S> {
    bot: Bot,
    bot_user_id: i64,
    chat_bot: ChatBot<L, M>,
    history: BufferStore<S>,
    settings: ProactiveSettings,
}

impl<L, M, S> ProactiveJob<L, M, S> {
    pub fn new(
        bot: Bot,
        bot_user_id: i64,
        chat_bot: ChatBot<L, M>,
        history: BufferStore<S>,
        settings: ProactiveSettings,
    ) -> Self {
        Self {
            bot,
            bot_user_id,
            chat_bot,
            history,
            settings,
        }
    }
}

impl<L, M, S> BackgroundJob for ProactiveJob<L, M, S>
where
    L: Llm + Clone + Send + Sync + 'static,
    M: Memory + Clone + Send + Sync + 'static,
    S: Storage + Clone + Send + Sync + 'static,
{
    fn spawn(self: Box<Self>) {
        let ProactiveJob {
            bot,
            bot_user_id,
            chat_bot,
            history,
            settings,
        } = *self;

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(settings.interval_minutes * 60));
            ticker.tick().await; // первый tick — немедленно; пропускаем, чтобы не писать сразу при старте

            loop {
                ticker.tick().await;

                if !roll_probability(settings.probability) {
                    tracing::debug!("proactive: skipped by probability roll");
                    continue;
                }

                let Some(chat_id) = pick_eligible_chat(&history, settings.min_inactivity_minutes).await
                else {
                    tracing::debug!("proactive: no eligible chat found");
                    continue;
                };

                async {
                    tracing::info!("proactive: giving the model a chance to write first");
                    let chat = ChannelId {
                        channel: TELEGRAM_CHANNEL,
                        id: chat_id.to_string(),
                    };

                    match chat_bot.run_proactive(chat).await {
                        Ok(Some(text)) => {
                            if let Err(err) = dispatch::send_and_record(
                                &bot,
                                bot_user_id,
                                &history,
                                ChatId(chat_id),
                                text,
                            )
                            .await
                            {
                                tracing::error!(%err, "proactive: failed to send message");
                            }
                        }
                        Ok(None) => {
                            tracing::debug!("proactive: model chose not to write");
                        }
                        Err(err) => {
                            tracing::error!(%err, "proactive turn failed");
                        }
                    }
                }
                .instrument(tracing::info_span!("proactive_job", chat_id))
                .await;
            }
        });
    }
}

fn roll_probability(p: f32) -> bool {
    rand::rng().random::<f32>() < p
}

async fn pick_eligible_chat<S>(history: &BufferStore<S>, min_inactivity_minutes: i64) -> Option<i64>
where
    S: Storage + Clone + Send + Sync + 'static,
{
    let mut activity = Vec::new();
    for chat_id in history.chat_ids().await {
        let last_activity = history.get(chat_id).await.and_then(|b| b.last_activity());
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
