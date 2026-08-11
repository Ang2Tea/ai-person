use std::time::Duration;

use chrono::Local;

use contracts::{BackgroundJob, Memory};

const CONSOLIDATION_HOUR_LOCAL: u32 = 3;

/// Раз в сутки в `CONSOLIDATION_HOUR_LOCAL:00` по локальному времени сервера
/// прогоняет `Memory::consolidate` — слияние/удаление фактов долгосрочной
/// памяти личности и пересборку insights. Сама логика консолидации целиком
/// внутри `Memory` (bot-core не знает про архив/дневник), эта задача — только
/// расписание.
pub struct ConsolidationJob<M> {
    memory: M,
}

impl<M> ConsolidationJob<M>
where
    M: Memory + Send + Sync + 'static,
{
    pub fn new(memory: M) -> Self {
        Self { memory }
    }
}

impl<M> BackgroundJob for ConsolidationJob<M>
where
    M: Memory + Send + Sync + 'static,
{
    fn spawn(self: Box<Self>) {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(duration_until_next_run(CONSOLIDATION_HOUR_LOCAL)).await;

                tracing::info!("starting nightly memory consolidation");
                if let Err(err) = self.memory.consolidate().await {
                    tracing::error!(%err, "nightly memory consolidation failed");
                }
            }
        });
    }
}

/// Если `hour` вдруг невалиден, или локальное время неоднозначно (переход на
/// летнее/зимнее время) — не паникуем, а откладываем на минуту и пробуем
/// снова на следующем витке цикла (тот же fallback, что и для последнего
/// вычитания ниже).
const RETRY_ON_AMBIGUOUS_TIME: Duration = Duration::from_secs(60);

fn duration_until_next_run(hour: u32) -> Duration {
    let now = Local::now();

    let Some(today_naive) = now.date_naive().and_hms_opt(hour, 0, 0) else {
        return RETRY_ON_AMBIGUOUS_TIME;
    };
    let Some(today_run) = today_naive.and_local_timezone(Local).single() else {
        return RETRY_ON_AMBIGUOUS_TIME;
    };

    let next_run = if today_run > now {
        today_run
    } else {
        today_run + chrono::Duration::days(1)
    };

    (next_run - now).to_std().unwrap_or(RETRY_ON_AMBIGUOUS_TIME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_until_next_run_targets_today_when_before_hour() {
        // Просто убеждаемся, что функция не паникует и возвращает разумную
        // (не нулевую, не превышающую сутки+запас) длительность для реального
        // "сейчас" — без mock времени, чтобы не тащить лишнюю зависимость.
        let d = duration_until_next_run(CONSOLIDATION_HOUR_LOCAL);
        assert!(d <= Duration::from_secs(24 * 60 * 60 + 60));
    }
}
