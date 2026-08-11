use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

use crate::errors::ToolError;

#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema параметров — сам формат параметров достаточно универсален
    /// между tool-calling API, поэтому не разворачивается в Rust-типы;
    /// провайдер-специфичен только конверт вокруг `ToolSpec` целиком.
    pub parameters: Value,
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn spec(&self) -> ToolSpec;
    fn call<'a>(
        &self,
        args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>>;

    /// Если `true`, вызов этого инструмента завершает ход модели молча
    /// (без финального текстового ответа) — см. `bot::run_tool_loop`.
    /// Дефолт `false` — большинство инструментов ход не завершают.
    fn ends_turn(&self) -> bool {
        false
    }
}
