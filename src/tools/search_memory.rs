use chrono::Utc;
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::pin::Pin;
use std::sync::Arc;

use crate::adapters::timeweb_client::TimewebClient;
use crate::errors::ToolError;
use crate::memory::{self, MemoryRecord, MemoryStore, Visibility};
use crate::settings::MemorySettings;
use crate::tools::{Tool, ToolContext};

pub struct SearchMemory {
    llm: TimewebClient,
    memory: MemoryStore,
    settings: MemorySettings,
    embedding_model: Arc<str>,
}

impl SearchMemory {
    pub fn new(
        llm: TimewebClient,
        memory: MemoryStore,
        settings: MemorySettings,
        embedding_model: Arc<str>,
    ) -> Self {
        Self {
            llm,
            memory,
            settings,
            embedding_model,
        }
    }
}

/// Запись видна из чата `chat_id` от лица пользователя `user_id`, если она
/// публичная, либо возникла в этом же чате, либо лично про этого пользователя
/// (даже если приватная и из другого чата).
fn is_visible(record: &MemoryRecord, chat_id: i64, user_id: i64) -> bool {
    record.visibility == Visibility::Public
        || record.origin_chat_id == chat_id
        || record.about_users.contains(&user_id)
}

impl<B: Send + Sync + 'static> Tool<B> for SearchMemory {
    fn name(&self) -> &str {
        "search_memory"
    }

    fn spec(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "search_memory",
                "description": "Найти в долгосрочной памяти факты, релевантные запросу. Ищет по всему архиву личности, не только по текущему чату — видны публичные факты, факты из этого же чата и приватные факты лично о текущем собеседнике.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Что ищем — краткое описание нужной информации",
                        },
                    },
                    "required": ["query"],
                },
            },
        })
    }

    fn call<'a>(
        &self,
        args: Value,
        ctx: &ToolContext<B>,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        let llm = self.llm.clone();
        let memory = self.memory.clone();
        let settings = self.settings.clone();
        let embedding_model = self.embedding_model.clone();
        let chat_id = ctx.chat_id.0;
        let user_id = ctx.user_id;

        Box::pin(async move {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::Failed("missing 'query' argument".to_owned()))?
                .to_owned();

            tracing::debug!(chat_id, user_id, query = %query, "search_memory: starting search");

            let query_embedding = llm
                .embed(&embedding_model, &query)
                .await
                .map_err(|e| ToolError::Failed(e.to_string()))?;

            let own_prefix = format!("{chat_id}--");
            let about_me_token = format!(",{user_id},");
            let records = memory
                .list_filtered(move |name| {
                    name.starts_with(&own_prefix)
                        || name.contains("--public--")
                        || name.contains(&about_me_token)
                })
                .await
                .map_err(|e| ToolError::Failed(e.to_string()))?;
            tracing::debug!(chat_id, record_count = records.len(), "search_memory: loaded records total");

            let mut matches: Vec<(f32, MemoryRecord)> = Vec::new();
            for record in records {
                if !is_visible(&record, chat_id, user_id) {
                    tracing::debug!(
                        chat_id,
                        record_id = %record.id,
                        origin_chat_id = record.origin_chat_id,
                        "search_memory: skipped record (not visible from here)"
                    );
                    continue;
                }

                let score = memory::cosine_similarity(&record.embedding, &query_embedding);
                tracing::debug!(
                    chat_id,
                    record_id = %record.id,
                    score,
                    threshold = settings.search_similarity_threshold,
                    "search_memory: scored record"
                );
                if score >= settings.search_similarity_threshold {
                    matches.push((score, record));
                }
            }

            matches.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
            matches.truncate(settings.search_result_limit);

            tracing::debug!(chat_id, match_count = matches.len(), "search_memory: finished");

            if matches.is_empty() {
                return Ok("в памяти ничего подходящего не найдено".to_owned());
            }

            let mut result = String::new();
            for (_, mut record) in matches {
                result.push_str(&format!(
                    "[уверенность: {}] {}",
                    record.confidence,
                    record.text.trim()
                ));
                result.push('\n');

                record.usage_count += 1;
                record.last_used = Some(Utc::now());
                if let Err(err) = memory.touch(&record).await {
                    tracing::warn!(%err, "failed to update memory record usage stats");
                }
            }

            Ok(result.trim().to_owned())
        })
    }
}
