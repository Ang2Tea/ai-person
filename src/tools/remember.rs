use serde_json::{Value, json};
use std::pin::Pin;

use crate::adapters::timeweb_client::TimewebClient;
use crate::errors::ToolError;
use crate::memory::{self, MemoryStore, NewFact, Visibility};
use crate::settings::MemorySettings;
use crate::tools::{Tool, ToolContext};

pub struct Remember {
    llm: TimewebClient,
    memory: MemoryStore,
    settings: MemorySettings,
}

impl Remember {
    pub fn new(llm: TimewebClient, memory: MemoryStore, settings: MemorySettings) -> Self {
        Self {
            llm,
            memory,
            settings,
        }
    }
}

impl<B: Send + Sync + 'static> Tool<B> for Remember {
    fn name(&self) -> &str {
        "remember"
    }

    fn spec(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "remember",
                "description": "Сохранить важный факт в долгосрочную память прямо сейчас, \
не дожидаясь фоновой архивации. Используй по прямой просьбе собеседника запомнить что-то, \
или когда сам считаешь факт важным и не хочешь полагаться на фоновую выгрузку — не для \
рутинной информации на каждую реплику.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Текст факта, который нужно запомнить",
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
                    "required": ["text"],
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
            let text = args
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::Failed("missing 'text' argument".to_owned()))?
                .to_owned();
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

            let folder = chat_id.to_string();
            let fact = NewFact {
                text,
                confidence,
                visibility,
                about_users,
            };
            let saved = memory::save_fact(
                &llm,
                &memory,
                &folder,
                fact,
                settings.dedup_similarity_threshold,
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
