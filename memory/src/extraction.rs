use contracts::{ChatMessage, Llm, Storage};

use crate::commitments::CommitmentsStore;
use crate::errors::MemoryError;
use crate::record::{NewFact, Visibility};
use crate::store::MemoryStore;
use crate::write::save_fact;

const EXTRACTION_SYSTEM_PROMPT: &str = include_str!("../../prompts/extraction_system.md");
const EXTRACTION_INSTRUCTION: &str = include_str!("../../prompts/extraction_instruction.md");
/// Маркер, разделяющий отдельные факты в ответе модели — должен совпадать с тем,
/// что просит использовать `EXTRACTION_INSTRUCTION`. Не `"---"`: это слишком
/// частая в обычном markdown/LLM-тексте последовательность, факт мог бы
/// случайно разбиться на куски, если бы содержал её сам.
const FACT_SEPARATOR: &str = "===FACT===";
/// Разделяет факты от обновлённого списка открытых задач/обещаний чата в
/// ответе модели — должен идти строго после всех `FACT_SEPARATOR`-блоков,
/// иначе список задач слипнется с текстом последнего факта.
const COMMITMENTS_SEPARATOR: &str = "===COMMITMENTS===";

fn parse_chunk(chunk: &str, origin_chat_id: i64) -> NewFact {
    let mut text_lines = Vec::new();
    let mut confidence = 0.0f32;
    let mut visibility = Visibility::Private;
    let mut about_users = Vec::new();

    for line in chunk.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("confidence:") {
            confidence = rest.trim().parse().unwrap_or(0.0);
        } else if let Some(rest) = trimmed.strip_prefix("visibility:") {
            visibility = if rest.trim() == "public" {
                Visibility::Public
            } else {
                Visibility::Private
            };
        } else if let Some(rest) = trimmed.strip_prefix("about_users:") {
            about_users = rest
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
        } else {
            text_lines.push(line);
        }
    }

    NewFact {
        text: text_lines.join("\n").trim().to_owned(),
        confidence,
        visibility,
        about_users,
        origin_chat_id,
    }
}

/// Отправляет отдельный (вне основного tool-calling цикла) запрос модели на
/// выделение фактов из уже готового транскрипта хода и сохраняет то, что
/// прошло дедуп. Обрезка истории канала до `keep_last_messages` — забота
/// вызывающего (`ChatBot`/воркер извлечения по простою), сама функция о
/// канале ничего не знает.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip(llm, memory, commitments, transcript), fields(transcript_len = transcript.len()))]
pub async fn maybe_extract<L, S>(
    llm: &L,
    memory: &MemoryStore<S>,
    commitments: &CommitmentsStore<S>,
    chat_id: i64,
    transcript: &str,
    dedup_similarity_threshold: f32,
    min_fact_length: usize,
    model: &str,
    embedding_model: &str,
) -> Result<(), MemoryError>
where
    L: Llm,
    S: Storage + Clone + Send + Sync + 'static,
{
    let existing_commitments = commitments.get().await;
    let commitments_context = existing_commitments.as_deref().unwrap_or("(пока пусто)");
    let messages = vec![
        ChatMessage::system(EXTRACTION_SYSTEM_PROMPT.trim()),
        ChatMessage::user(format!(
            "Текущий общий список открытых задач/обещаний (по всем чатам):\n{commitments_context}\n\n{transcript}\n\n{EXTRACTION_INSTRUCTION}"
        )),
    ];

    let completion = llm.chat(model, &messages, &[]).await?;

    if let Some(content) = completion.message.content {
        // Сначала отделяем блок задач от фактов — если резать сразу по
        // FACT_SEPARATOR по всему content, хвост с задачами прилипнет к
        // тексту последнего факта.
        let (facts_part, new_commitments) = match content.split_once(COMMITMENTS_SEPARATOR) {
            Some((facts, commitments)) => (facts, Some(commitments.trim().to_owned())),
            None => (content.as_str(), None),
        };

        for chunk in facts_part.split(FACT_SEPARATOR) {
            let fact = parse_chunk(chunk, chat_id);
            if fact.text.len() < min_fact_length {
                continue;
            }

            if let Err(err) =
                save_fact(llm, memory, fact, dedup_similarity_threshold, embedding_model).await
            {
                tracing::error!(%err, "failed to save extracted fact");
            }
        }

        if let Some(new_commitments) = new_commitments
            && let Err(err) = commitments.set(new_commitments).await
        {
            tracing::error!(%err, "failed to save updated commitments");
        }
    }

    Ok(())
}
