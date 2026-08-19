use serde::Serialize;

/// Request-side wire-формат для vision-запроса — обычный chat completion, но
/// `content` сообщения не плоская строка (как в `wire::WireMessage`), а
/// массив частей текст+картинка, поэтому отдельный набор типов, а не
/// переиспользование `WireMessage`.
#[derive(Debug, Serialize)]
pub struct VisionRequest<'a> {
    pub model: &'a str,
    pub messages: Vec<VisionMessage>,
    pub temperature: f32,
}

#[derive(Debug, Serialize)]
pub struct VisionMessage {
    pub role: &'static str,
    pub content: Vec<VisionContentPart>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum VisionContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrlData },
}

#[derive(Debug, Serialize)]
pub struct ImageUrlData {
    pub url: String,
}
