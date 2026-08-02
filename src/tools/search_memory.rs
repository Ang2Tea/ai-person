use chrono::Utc;
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::pin::Pin;

use crate::adapters::timeweb_client::TimewebClient;
use crate::errors::ToolError;
use crate::memory::{self, MemoryRecord, MemoryStore, Visibility};
use crate::settings::MemorySettings;
use crate::tools::{Tool, ToolContext};

pub struct SearchMemory {
    llm: TimewebClient,
    memory: MemoryStore,
    settings: MemorySettings,
}

impl SearchMemory {
    pub fn new(llm: TimewebClient, memory: MemoryStore, settings: MemorySettings) -> Self {
        Self {
            llm,
            memory,
            settings,
        }
    }
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
                "description": "Найти в долгосрочной памяти факты, релевантные запросу.",
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
        let chat_id = ctx.chat_id.0;

        Box::pin(async move {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::Failed("missing 'query' argument".to_owned()))?
                .to_owned();

            let query_embedding = llm
                .embed(memory::EMBEDDING_MODEL, &query)
                .await
                .map_err(|e| ToolError::Failed(e.to_string()))?;

            let own_folder = chat_id.to_string();
            let is_group = chat_id < 0;

            let mut matches: Vec<(f32, String, MemoryRecord)> = Vec::new();
            for folder in memory.candidate_folders(chat_id) {
                let records = memory
                    .list(&folder)
                    .map_err(|e| ToolError::Failed(e.to_string()))?;

                for record in records {
                    // В группе записи из чужих (не собственной групповой) папок,
                    // помеченные private, не должны попадать в кандидаты вообще —
                    // фильтрация в коде, не полагаемся на модель.
                    if is_group && folder != own_folder && record.visibility == Visibility::Private
                    {
                        continue;
                    }

                    let score = memory::cosine_similarity(&record.embedding, &query_embedding);
                    if score >= settings.search_similarity_threshold {
                        matches.push((score, folder.clone(), record));
                    }
                }
            }

            matches.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
            matches.truncate(settings.search_result_limit);

            if matches.is_empty() {
                return Ok("в памяти ничего подходящего не найдено".to_owned());
            }

            let mut result = String::new();
            for (_, folder, mut record) in matches {
                result.push_str(record.text.trim());
                result.push('\n');

                record.usage_count += 1;
                record.last_used = Some(Utc::now());
                if let Err(err) = memory.touch(&folder, &record) {
                    tracing::warn!(%err, "failed to update memory record usage stats");
                }
            }

            Ok(result.trim().to_owned())
        })
    }
}
