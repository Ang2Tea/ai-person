use serde::Deserialize;
use serde::Serialize;

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
    pub content: String,
    pub role: String,
    #[serde(rename = "reasoning_content")]
    pub reasoning_content: String,
    #[serde(rename = "provider_specific_fields")]
    pub provider_specific_fields: ProviderSpecificFields,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSpecificFields {
    #[serde(rename = "reasoning_content")]
    pub reasoning_content: String,
}
