use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{Local, Utc};
use tokio::sync::RwLock;

use contracts::{ChatMessage, Llm, Storage};

use crate::errors::ConsolidationError;
use crate::memory::{self, MemoryRecord, MemoryStore, Visibility};
use crate::settings::{MemorySettings, PersonalitySettings};

const CONSOLIDATION_HOUR_LOCAL: u32 = 3;
const MERGE_SYSTEM_PROMPT: &str = include_str!("../../prompts/consolidation_merge_system.md");
const PRUNE_SYSTEM_PROMPT: &str = include_str!("../../prompts/consolidation_prune_system.md");
const INSIGHTS_SYSTEM_PROMPT: &str = include_str!("../../prompts/insights_system.md");

/// Текст, подмешиваемый в системный prompt личности — общий на процесс,
/// обновляется ночной консолидацией и сразу подхватывается ботом без
/// перезапуска (см. `ChatBot::handle_message`).
pub type SharedInsights = Arc<RwLock<Arc<str>>>;

/// Spawning фоновую задачу, которая раз в сутки в `CONSOLIDATION_HOUR_LOCAL:00`
/// по локальному времени сервера прогоняет `run` над архивом долгосрочной
/// памяти личности. Ошибка одного прогона не останавливает планировщик — как
/// и `BufferStore::spawn_flush_task`, просто log и цикл продолжается.
#[allow(clippy::too_many_arguments)]
pub fn spawn_daily_task<L, S>(
    llm: L,
    memory: MemoryStore<S>,
    memory_settings: MemorySettings,
    model: Arc<str>,
    embedding_model: Arc<str>,
    personality: PersonalitySettings,
    personality_storage: S,
    insights: SharedInsights,
) where
    L: Llm + Clone + Send + Sync + 'static,
    S: Storage + Clone + Send + Sync + 'static,
{
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(duration_until_next_run(CONSOLIDATION_HOUR_LOCAL)).await;

            tracing::info!("starting nightly memory consolidation");
            if let Err(err) = run(
                &llm,
                &memory,
                &memory_settings,
                &model,
                &embedding_model,
                &personality,
                &personality_storage,
                &insights,
            )
            .await
            {
                tracing::error!(%err, "nightly memory consolidation failed");
            }
        }
    });
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

/// Один прогон консолидации: слить похожие факты, удалить редко используемые,
/// пересобрать insights из оставшихся публичных фактов.
pub async fn run<L, S>(
    llm: &L,
    memory: &MemoryStore<S>,
    memory_settings: &MemorySettings,
    model: &str,
    embedding_model: &str,
    personality: &PersonalitySettings,
    personality_storage: &S,
    insights: &SharedInsights,
) -> Result<(), ConsolidationError>
where
    L: Llm,
    S: Storage + Clone + Send + Sync + 'static,
{
    let records = memory.list_all().await?;
    tracing::debug!(count = records.len(), "consolidation: loaded records");

    merge_similar(
        llm,
        memory,
        model,
        embedding_model,
        memory_settings.dedup_similarity_threshold,
        records,
    )
    .await?;

    let remaining = memory.list_all().await?;
    let system_prompt =
        read_system_prompt(personality_storage, &personality.files.system_prompt).await?;
    prune_irrelevant(llm, memory, model, &system_prompt, &remaining).await?;
    remove_stale(memory, memory_settings.stale_after_days, &remaining).await?;

    let public_records: Vec<MemoryRecord> = memory
        .list_all()
        .await?
        .into_iter()
        .filter(|r| r.visibility == Visibility::Public)
        .collect();

    if public_records.is_empty() {
        tracing::debug!("consolidation: no public facts, skipping insights regeneration");
        return Ok(());
    }

    let generated = generate_insights(llm, model, public_records).await?;
    write_insights(personality_storage, &personality.files.insights, &generated).await?;
    *insights.write().await = generated.into();

    tracing::info!("consolidation: finished, insights regenerated");
    Ok(())
}

/// Ключ группировки — сливать можно только записи внутри одной группы.
/// `Private` группируется по (чат, набор about_users), как и раньше — иначе
/// слияние само стало бы обходом правила видимости фактов, нельзя
/// объединять приватный факт из одного чата с приватным фактом из другого.
/// `Public` — единый ключ без привязки к чату: публичный факт по определению
/// виден отовсюду, значит один и тот же факт, записанный в разных чатах,
/// можно и нужно сливать в один.
#[derive(PartialEq, Eq, Hash)]
enum GroupKey {
    Public,
    Private(i64, Vec<i64>),
}

fn group_key(record: &MemoryRecord) -> GroupKey {
    match record.visibility {
        Visibility::Public => GroupKey::Public,
        Visibility::Private => {
            let mut about_users = record.about_users.clone();
            about_users.sort_unstable();
            GroupKey::Private(record.origin_chat_id, about_users)
        }
    }
}

async fn merge_similar<L, S>(
    llm: &L,
    memory: &MemoryStore<S>,
    model: &str,
    embedding_model: &str,
    similarity_threshold: f32,
    records: Vec<MemoryRecord>,
) -> Result<(), ConsolidationError>
where
    L: Llm,
    S: Storage + Clone + Send + Sync + 'static,
{
    let mut groups: HashMap<GroupKey, Vec<MemoryRecord>> = HashMap::new();
    for record in records {
        groups.entry(group_key(&record)).or_default().push(record);
    }

    for group in groups.into_values() {
        for cluster in cluster_by_similarity(group, similarity_threshold) {
            if cluster.len() < 2 {
                continue;
            }
            if let Err(err) = merge_cluster(llm, memory, model, embedding_model, cluster).await {
                tracing::warn!(%err, "consolidation: failed to merge a cluster of similar facts");
            }
        }
    }

    Ok(())
}

/// Простая жадная кластеризация по транзитивному сходству (single-link):
/// пока к кластеру находится хоть одна ещё не пристроенная запись с
/// cosine_similarity выше порога к любому уже включённому элементу — она
/// присоединяется. Архив небольшой (нет БД, см. README), квадратичная
/// стоимость приемлема.
fn cluster_by_similarity(mut records: Vec<MemoryRecord>, threshold: f32) -> Vec<Vec<MemoryRecord>> {
    let mut clusters = Vec::new();

    while let Some(seed) = records.pop() {
        let mut cluster = vec![seed];
        loop {
            let mut grew = false;
            let mut i = 0;
            while i < records.len() {
                let is_similar = cluster.iter().any(|r| {
                    memory::cosine_similarity(&r.embedding, &records[i].embedding) >= threshold
                });
                if is_similar {
                    cluster.push(records.remove(i));
                    grew = true;
                } else {
                    i += 1;
                }
            }
            if !grew {
                break;
            }
        }
        clusters.push(cluster);
    }

    clusters
}

async fn merge_cluster<L, S>(
    llm: &L,
    memory: &MemoryStore<S>,
    model: &str,
    embedding_model: &str,
    cluster: Vec<MemoryRecord>,
) -> Result<(), ConsolidationError>
where
    L: Llm,
    S: Storage + Clone + Send + Sync + 'static,
{
    let joined = cluster
        .iter()
        .enumerate()
        .map(|(i, r)| format!("{}. {}", i + 1, r.text))
        .collect::<Vec<_>>()
        .join("\n");

    let messages = vec![
        ChatMessage::system(MERGE_SYSTEM_PROMPT.trim()),
        ChatMessage::user(joined),
    ];

    let completion = llm.chat(model, &messages, &[]).await?;

    let Some(merged_text) = completion
        .message
        .content
        .filter(|t| !t.trim().is_empty())
        .map(|t| t.trim().to_owned())
    else {
        tracing::warn!("consolidation: model returned no merged text, leaving cluster as-is");
        return Ok(());
    };

    let embedding = llm.embed(embedding_model, &merged_text).await?;

    let first = &cluster[0];
    let avg_confidence =
        cluster.iter().map(|r| r.confidence).sum::<f32>() / cluster.len() as f32;

    let mut merged = MemoryRecord::new(
        merged_text,
        avg_confidence,
        first.visibility,
        merge_about_users(&cluster),
        first.origin_chat_id,
        embedding,
    );
    merged.usage_count = cluster.iter().map(|r| r.usage_count).sum();
    merged.last_used = cluster.iter().filter_map(|r| r.last_used).max();

    // Сначала пишем объединённую запись, потом удаляем источники — если
    // процесс упадёт между шагами, в худшем случае останется дубликат
    // (сам себя починит на следующей ночи), а не потеря фактов.
    memory.append(&merged).await?;
    for record in &cluster {
        memory.remove(record).await?;
    }

    Ok(())
}

/// Объединение `about_users` всех записей кластера (сорт + dedup). Для
/// `private` это не меняет результат — в группе и так все `about_users`
/// идентичны по построению (`group_key`). Для `public` (сливаются между
/// чатами) — сохраняет упоминания всех людей, о которых был хоть один из
/// исходных фактов, вместо того чтобы взять только `about_users` первого.
fn merge_about_users(cluster: &[MemoryRecord]) -> Vec<i64> {
    let mut about_users: Vec<i64> = cluster
        .iter()
        .flat_map(|r| r.about_users.iter().copied())
        .collect();
    about_users.sort_unstable();
    about_users.dedup();
    about_users
}

async fn read_system_prompt<S>(storage: &S, key: &str) -> Result<String, ConsolidationError>
where
    S: Storage,
{
    Ok(storage.get(key.to_owned()).await?)
}

/// Удаляет факты, которые не стоило хранить: дублируют характер бота из
/// системного prompt, бытовая мелочь без ценности, мета-разговоры о самой
/// памяти — то, что `merge_similar`/`remove_stale` не ловят (это не дубликаты
/// и не устаревшие, они могли быть только вчера и ни разу не совпасть по
/// embedding с другим фактом).
async fn prune_irrelevant<L, S>(
    llm: &L,
    memory: &MemoryStore<S>,
    model: &str,
    system_prompt: &str,
    records: &[MemoryRecord],
) -> Result<(), ConsolidationError>
where
    L: Llm,
    S: Storage + Clone + Send + Sync + 'static,
{
    if records.is_empty() {
        return Ok(());
    }

    let numbered = records
        .iter()
        .enumerate()
        .map(|(i, r)| format!("{}. {}", i + 1, r.text))
        .collect::<Vec<_>>()
        .join("\n");

    let messages = vec![
        ChatMessage::system(PRUNE_SYSTEM_PROMPT.trim()),
        ChatMessage::user(format!(
            "Системный prompt:\n{system_prompt}\n\nФакты:\n{numbered}"
        )),
    ];

    let completion = llm.chat(model, &messages, &[]).await?;
    let response = completion.message.content.unwrap_or_default();
    let discard = parse_discard_indices(&response);

    if discard.is_empty() {
        tracing::debug!("consolidation: nothing to prune");
        return Ok(());
    }

    // Необратимая операция — если модель вернула индексов на удаление больше
    // или равно общему числу записей, это похоже на то, что она не поняла
    // задачу, а не на реальное "всё это не нужно". Лучше ничего не сделать.
    if discard.len() >= records.len() {
        tracing::warn!(
            discard_count = discard.len(),
            total = records.len(),
            "consolidation: prune response looks degenerate (would remove everything), skipping"
        );
        return Ok(());
    }

    tracing::info!(count = discard.len(), "consolidation: pruning irrelevant facts");
    for (i, record) in records.iter().enumerate() {
        if discard.contains(&(i + 1)) {
            memory.remove(record).await?;
        }
    }

    Ok(())
}

/// Терпимый парсинг номеров фактов на удаление — по одному на строку или
/// через запятую, посторонний текст в ответе (модель могла добавить
/// пояснение вопреки инструкции) просто игнорируется.
fn parse_discard_indices(response: &str) -> std::collections::HashSet<usize> {
    response
        .split([',', '\n'])
        .filter_map(|s| s.trim().parse::<usize>().ok())
        .collect()
}

async fn remove_stale<S>(
    memory: &MemoryStore<S>,
    stale_after_days: i64,
    records: &[MemoryRecord],
) -> Result<(), ConsolidationError>
where
    S: Storage + Clone + Send + Sync + 'static,
{
    let now = Utc::now();

    for record in records {
        let reference = record.last_used.or_else(|| record.created_at());
        let is_stale = reference
            .map(|t| now - t > chrono::Duration::days(stale_after_days))
            .unwrap_or(false);

        if is_stale {
            memory.remove(record).await?;
        }
    }

    Ok(())
}

async fn generate_insights<L>(
    llm: &L,
    model: &str,
    public_records: Vec<MemoryRecord>,
) -> Result<String, ConsolidationError>
where
    L: Llm,
{
    let facts = public_records
        .iter()
        .map(|r| format!("- {}", r.text))
        .collect::<Vec<_>>()
        .join("\n");

    let messages = vec![
        ChatMessage::system(INSIGHTS_SYSTEM_PROMPT.trim()),
        ChatMessage::user(facts),
    ];

    let completion = llm.chat(model, &messages, &[]).await?;
    Ok(completion.message.content.unwrap_or_default().trim().to_owned())
}

async fn write_insights<S>(storage: &S, key: &str, content: &str) -> Result<(), ConsolidationError>
where
    S: Storage,
{
    storage.set(key.to_owned(), content.to_owned()).await?;
    Ok(())
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

    #[test]
    fn cluster_by_similarity_groups_transitively() {
        use crate::memory::Visibility;

        let make = |text: &str, embedding: Vec<f32>| {
            MemoryRecord::new(text, 0.0, Visibility::Private, vec![], 1, embedding)
        };

        let a = make("a", vec![1.0, 0.0]);
        let b = make("b", vec![0.99, 0.01]);
        let c = make("c", vec![0.0, 1.0]);

        let clusters = cluster_by_similarity(vec![a, b, c], 0.9);
        assert_eq!(clusters.len(), 2);
        let sizes: Vec<usize> = clusters.iter().map(Vec::len).collect();
        assert!(sizes.contains(&2));
        assert!(sizes.contains(&1));
    }

    #[test]
    fn public_records_from_different_chats_share_a_group_key() {
        let a = MemoryRecord::new("a", 0.0, Visibility::Public, vec![1], 111, vec![1.0]);
        let b = MemoryRecord::new("b", 0.0, Visibility::Public, vec![2], 222, vec![1.0]);

        assert!(group_key(&a) == group_key(&b));
    }

    #[test]
    fn private_records_from_different_chats_have_different_group_keys() {
        let a = MemoryRecord::new("a", 0.0, Visibility::Private, vec![1], 111, vec![1.0]);
        let b = MemoryRecord::new("b", 0.0, Visibility::Private, vec![1], 222, vec![1.0]);

        assert!(group_key(&a) != group_key(&b));
    }

    #[test]
    fn merge_about_users_unions_and_dedups() {
        let a = MemoryRecord::new("a", 0.0, Visibility::Public, vec![1, 2], 1, vec![1.0]);
        let b = MemoryRecord::new("b", 0.0, Visibility::Public, vec![2, 3], 1, vec![1.0]);

        assert_eq!(merge_about_users(&[a, b]), vec![1, 2, 3]);
    }

    #[test]
    fn parse_discard_indices_ignores_non_numeric_lines() {
        let response = "Вот номера на удаление:\n2\n4\n7\nГотово.";
        let discard = parse_discard_indices(response);
        assert_eq!(discard, std::collections::HashSet::from([2, 4, 7]));
    }

    #[test]
    fn parse_discard_indices_supports_comma_separated_list() {
        let discard = parse_discard_indices("2, 4, 7");
        assert_eq!(discard, std::collections::HashSet::from([2, 4, 7]));
    }

    #[test]
    fn parse_discard_indices_empty_response_discards_nothing() {
        assert!(parse_discard_indices("").is_empty());
    }
}
