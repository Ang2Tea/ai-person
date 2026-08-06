use contracts::{Llm, Storage};

use crate::errors::MemoryError;
use crate::memory::record::{MemoryRecord, NewFact};
use crate::memory::similarity::cosine_similarity;
use crate::memory::store::MemoryStore;

/// Эмбеддинг + дедуп + запись факта в архив. Возвращает `Ok(true)`, если факт
/// сохранён, и `Ok(false)`, если в том же чате (`fact.origin_chat_id`) уже есть
/// достаточно похожая запись (факт отброшен как дубликат). Дедуп скопирован по
/// чату — иначе слегка похожий факт из разговора с одним человеком мог бы
/// задедупить не связанный факт из разговора с другим.
pub async fn save_fact<L, S>(
    llm: &L,
    memory: &MemoryStore<S>,
    fact: NewFact,
    dedup_threshold: f32,
    embedding_model: &str,
) -> Result<bool, MemoryError>
where
    L: Llm,
    S: Storage + Clone + Send + Sync + 'static,
{
    let embedding = llm.embed(embedding_model, &fact.text).await?;

    let origin_prefix = format!("{}--", fact.origin_chat_id);
    let existing = memory
        .list_filtered(move |name| name.starts_with(&origin_prefix))
        .await?;
    let is_duplicate = existing
        .iter()
        .any(|r| cosine_similarity(&r.embedding, &embedding) >= dedup_threshold);

    if is_duplicate {
        return Ok(false);
    }

    let record = MemoryRecord::new(
        fact.text,
        fact.confidence,
        fact.visibility,
        fact.about_users,
        fact.origin_chat_id,
        embedding,
    );
    memory.append(&record).await?;
    Ok(true)
}
