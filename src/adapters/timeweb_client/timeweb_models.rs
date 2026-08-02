use serde::Deserialize;
use serde::Serialize;

use crate::contracts::ToolCall;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResponse {
    pub id: String,
    pub created: i64,
    pub model: String,
    pub object: String,
    #[serde(rename = "system_fingerprint")]
    pub system_fingerprint: String,
    pub choices: Vec<Choice>,
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
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSpecificFields {
    #[serde(rename = "reasoning_content")]
    pub reasoning_content: String,
}
