use contracts::{ChatMessage, ToolCall, ToolSpec};
use serde::{Deserialize, Serialize};

/// JSON-форма сообщения в запросе к Timeweb — `contracts::ChatMessage` не
/// сериализуется напрямую (это домен-тип, общий с `bot-core`, а не формат
/// конкретного API), поэтому здесь отдельная wire-структура и явный перевод.
#[derive(Debug, Serialize)]
pub struct WireMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_calls: Option<Vec<WireToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call_id: Option<String>,
}

impl From<&ChatMessage> for WireMessage {
    fn from(message: &ChatMessage) -> Self {
        Self {
            role: message.role.clone(),
            content: message.content.clone(),
            tool_calls: message
                .tool_calls
                .as_ref()
                .map(|calls| calls.iter().map(WireToolCall::from).collect()),
            tool_call_id: message.tool_call_id.clone(),
        }
    }
}

/// Та же JSON-форма нужна и в запросе (эхо ранее полученного вызова
/// инструмента), и в ответе (модель сообщает, что хочет вызвать инструмент) —
/// поэтому одна структура с обоими направлениями сериализации.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: WireFunctionCall,
}

impl From<&ToolCall> for WireToolCall {
    fn from(call: &ToolCall) -> Self {
        Self {
            id: call.id.clone(),
            kind: call.kind.clone(),
            function: WireFunctionCall {
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            },
        }
    }
}

impl From<WireToolCall> for ToolCall {
    fn from(call: WireToolCall) -> Self {
        Self {
            id: call.id,
            kind: call.kind,
            name: call.function.name,
            arguments: call.function.arguments,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireFunctionCall {
    pub name: String,
    pub arguments: String,
}

/// JSON-форма описания инструмента в запросе (конверт `{"type": "function",
/// "function": {...}}`) — то самое провайдер-специфичное обёртывание вокруг
/// `contracts::ToolSpec`, о котором сказано в его doc-comment'е.
#[derive(Debug, Serialize)]
pub struct WireToolSpec {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: WireFunctionSpec,
}

#[derive(Debug, Serialize)]
pub struct WireFunctionSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl From<&ToolSpec> for WireToolSpec {
    fn from(spec: &ToolSpec) -> Self {
        Self {
            kind: "function",
            function: WireFunctionSpec {
                name: spec.name.clone(),
                description: spec.description.clone(),
                parameters: spec.parameters.clone(),
            },
        }
    }
}
