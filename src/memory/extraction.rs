use crate::adapters::timeweb_client::TimewebClient;
use crate::buffer::BufferStore;
use crate::contracts::{BufferStorage, ChatMessage};
use crate::errors::MemoryError;
use crate::memory::{save_fact, MemoryStore, NewFact, Visibility};
use crate::settings::MemorySettings;

const MODEL: &str = "deepseek/deepseek-v4-flash";

const EXTRACTION_INSTRUCTION: &str = include_str!("../../extraction_instruction.md");

fn parse_chunk(chunk: &str) -> NewFact {
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
    }
}

/// Отправляет отдельный (вне основного tool-calling цикла) запрос модели на
/// выделение фактов из текущего буфера чата, сохраняет то, что прошло дедуп, и
/// обрезает (не очищает) буфер до последних `keep_last_messages` сообщений.
pub async fn maybe_extract<B>(
    llm: &TimewebClient,
    memory: &MemoryStore,
    buffer: &BufferStore<B>,
    chat_id: i64,
    settings: &MemorySettings,
) -> Result<(), MemoryError>
where
    B: BufferStorage + Clone + Send + Sync + 'static,
{
    let Some(chat_buffer) = buffer.get(chat_id).await else {
        return Ok(());
    };

    let transcript = chat_buffer.to_transcript();
    let messages = vec![
        ChatMessage::system(
            "Ты — аналитик, который выделяет факты из истории переписки для долгосрочного \
             архива. Отвечай только фактами и метаданными, без художественного текста.",
        ),
        ChatMessage::user(format!("{transcript}\n\n{EXTRACTION_INSTRUCTION}")),
    ];

    let completion = llm.chat(MODEL, &messages, &[]).await?;

    if let Some(content) = completion.message.content {
        let folder = chat_id.to_string();

        for chunk in content.split("---") {
            let fact = parse_chunk(chunk);
            if fact.text.len() < settings.min_fact_length {
                continue;
            }

            if let Err(err) =
                save_fact(llm, memory, &folder, fact, settings.dedup_similarity_threshold).await
            {
                tracing::error!(chat_id, %err, "failed to save extracted fact");
            }
        }
    }

    buffer
        .truncate_keep_last(chat_id, settings.keep_last_messages)
        .await;

    Ok(())
}
