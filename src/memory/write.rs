use crate::adapters::timeweb_client::TimewebClient;
use crate::errors::MemoryError;
use crate::memory::record::{MemoryRecord, NewFact};
use crate::memory::similarity::cosine_similarity;
use crate::memory::store::MemoryStore;
use crate::memory::EMBEDDING_MODEL;

/// Эмбеддинг + дедуп + запись факта в архив. Возвращает `Ok(true)`, если факт
/// сохранён, и `Ok(false)`, если в `folder` уже есть достаточно похожая запись
/// (факт отброшен как дубликат).
pub async fn save_fact(
    llm: &TimewebClient,
    memory: &MemoryStore,
    folder: &str,
    fact: NewFact,
    dedup_threshold: f32,
) -> Result<bool, MemoryError> {
    let embedding = llm.embed(EMBEDDING_MODEL, &fact.text).await?;

    let existing = memory.list(folder)?;
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
        embedding,
    );
    memory.append(folder, &record)?;
    Ok(true)
}
