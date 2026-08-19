use serde::Deserialize;
use serde::Serialize;

use crate::wire::WireToolCall;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResponse {
    pub id: String,
    pub created: i64,
    pub model: String,
    pub object: String,
    // Не все модели, проксируемые Timeweb, возвращают это поле (например,
    // dashscope/qwen3.7-plus его не присылает вовсе) — без `default` это
    // валило десериализацию ответа целиком, из-за чего бот не мог получить
    // от такой модели вообще никакого ответа. Поле нигде не используется
    // ниже, но оставляем для отладки/логов.
    #[serde(rename = "system_fingerprint", default)]
    pub system_fingerprint: String,
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Usage,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    #[serde(rename = "prompt_tokens", default)]
    pub prompt_tokens: u32,
    #[serde(rename = "completion_tokens", default)]
    pub completion_tokens: u32,
    #[serde(rename = "total_tokens", default)]
    pub total_tokens: u32,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    #[serde(rename = "finish_reason")]
    pub finish_reason: String,
    pub index: i64,
    pub message: Message,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    #[serde(default)]
    pub content: Option<String>,
    pub role: String,
    #[serde(rename = "reasoning_content", default)]
    pub reasoning_content: String,
    #[serde(rename = "provider_specific_fields", default)]
    pub provider_specific_fields: ProviderSpecificFields,
    #[serde(rename = "tool_calls", default)]
    pub tool_calls: Option<Vec<WireToolCall>>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSpecificFields {
    #[serde(rename = "reasoning_content")]
    pub reasoning_content: String,
}
