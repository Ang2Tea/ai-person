use contracts::{Llm, Storage, Tool, ToolError, ToolSpec};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::memory::{self, MemoryStore, NewFact, Visibility};
use crate::settings::MemorySettings;

pub struct Remember<L, B> {
    llm: L,
    memory: MemoryStore<B>,
    settings: MemorySettings,
    embedding_model: Arc<str>,
}

impl<L, B> Remember<L, B> {
    pub fn new(
        llm: L,
        memory: MemoryStore<B>,
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

impl<L, B> Tool for Remember<L, B>
where
    L: Llm + Clone + Send + Sync + 'static,
    B: Storage + Clone + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        "remember"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "remember".to_owned(),
            description: "Сохранить важный факт в долгосрочную память прямо сейчас, \
не дожидаясь фоновой архивации. Используй по прямой просьбе собеседника запомнить что-то, \
или когда сам считаешь факт важным и не хочешь полагаться на фоновую выгрузку — не для \
рутинной информации на каждую реплику."
                .to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Текст факта, который нужно запомнить",
                    },
                    "chat_id": {
                        "type": "integer",
                        "description": "id чата, к которому относится факт",
                    },
                    "confidence": {
                        "type": "number",
                        "description": "-1 (заведомая ложь) .. 1 (точно подтверждено), 0 по умолчанию",
                    },
                    "visibility": {
                        "type": "string",
                        "enum": ["private", "public"],
                        "description": "private по умолчанию, public — только если факт точно нейтрален",
                    },
                    "about_users": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Telegram user_id, если факт о конкретных людях в группе",
                    },
                },
                "required": ["text", "chat_id"],
            }),
        }
    }

    fn call<'a>(
        &self,
        args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        let llm = self.llm.clone();
        let memory = self.memory.clone();
        let settings = self.settings.clone();
        let embedding_model = self.embedding_model.clone();

        Box::pin(async move {
            let text = args
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::Failed("missing 'text' argument".to_owned()))?
                .to_owned();
            let chat_id = args
                .get("chat_id")
                .and_then(Value::as_i64)
                .ok_or_else(|| ToolError::Failed("missing 'chat_id' argument".to_owned()))?;
            let confidence = args
                .get("confidence")
                .and_then(Value::as_f64)
                .unwrap_or(0.0) as f32;
            let visibility = match args.get("visibility").and_then(Value::as_str) {
                Some("public") => Visibility::Public,
                _ => Visibility::Private,
            };
            let about_users = args
                .get("about_users")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default();

            let fact = NewFact {
                text,
                confidence,
                visibility,
                about_users,
                origin_chat_id: chat_id,
            };
            let saved = memory::save_fact(
                &llm,
                &memory,
                fact,
                settings.dedup_similarity_threshold,
                &embedding_model,
            )
            .await
            .map_err(|e| ToolError::Failed(e.to_string()))?;

            Ok(if saved {
                "запомнено".to_owned()
            } else {
                "уже было похожее, не сохранено".to_owned()
            })
        })
    }
}
