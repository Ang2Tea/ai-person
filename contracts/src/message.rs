use crate::ToolCall;

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self::new("system", Some(content.into()), None)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new("user", Some(content.into()), None)
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::new("tool", Some(content.into()), Some(tool_call_id.into()))
    }

    fn new(role: &str, content: Option<String>, tool_call_id: Option<String>) -> Self {
        Self {
            role: String::from(role),
            content,
            tool_calls: None,
            tool_call_id,
        }
    }
}
