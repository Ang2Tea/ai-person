#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub kind: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}
