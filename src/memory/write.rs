use crate::adapters::timeweb_client::TimewebClient;
use crate::errors::MemoryError;
use crate::memory::EMBEDDING_MODEL;
use crate::memory::record::{MemoryRecord, NewFact};
use crate::memory::similarity::cosine_similarity;
use crate::memory::store::MemoryStore;

/// Эмбеддинг + дедуп + запись факта в архив. Возвращает `Ok(true)`, если факт
/// сохранён, и `Ok(false)`, если в том же чате (`fact.origin_chat_id`) уже есть
/// достаточно похожая запись (факт отброшен как дубликат). Дедуп скопирован по
/// чату — иначе слегка похожий факт из разговора с одним человеком мог бы
/// задедупить не связанный факт из разговора с другим.
pub async fn save_fact(
    llm: &TimewebClient,
    memory: &MemoryStore,
    fact: NewFact,
    dedup_threshold: f32,
) -> Result<bool, MemoryError> {
    let embedding = llm.embed(EMBEDDING_MODEL, &fact.text).await?;

    let existing = memory.list_all()?;
    let is_duplicate = existing
        .iter()
        .filter(|r| r.origin_chat_id == fact.origin_chat_id)
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
    memory.append(&record)?;
    Ok(true)
}
