#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub arguments: String,
}
