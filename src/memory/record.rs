use chrono::{DateTime, Utc};
use serde_yaml::Value;

use crate::errors::MemoryError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Public,
}

impl Visibility {
    fn as_str(&self) -> &'static str {
        match self {
            Visibility::Private => "private",
            Visibility::Public => "public",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "public" => Visibility::Public,
            _ => Visibility::Private,
        }
    }
}

/// Факт, ожидающий эмбеддинга и сохранения — общий вход для фонового извлечения
/// и проактивного инструмента `remember`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewFact {
    pub text: String,
    pub confidence: f32,
    pub visibility: Visibility,
    pub about_users: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryRecord {
    pub id: String,
    pub confidence: f32,
    pub visibility: Visibility,
    pub about_users: Vec<i64>,
    pub last_used: Option<DateTime<Utc>>,
    pub usage_count: u32,
    pub embedding: Vec<f32>,
    pub text: String,
}

impl MemoryRecord {
    pub fn new(
        text: impl Into<String>,
        confidence: f32,
        visibility: Visibility,
        about_users: Vec<i64>,
        embedding: Vec<f32>,
    ) -> Self {
        Self {
            id: Utc::now().timestamp().to_string(),
            confidence,
            visibility,
            about_users,
            last_used: None,
            usage_count: 0,
            embedding,
            text: text.into(),
        }
    }

    pub fn to_markdown(&self) -> String {
        let mut mapping = serde_yaml::Mapping::new();
        mapping.insert(Value::String("id".into()), Value::String(self.id.clone()));
        mapping.insert(
            Value::String("confidence".into()),
            Value::Number((self.confidence as f64).into()),
        );
        mapping.insert(
            Value::String("visibility".into()),
            Value::String(self.visibility.as_str().into()),
        );
        mapping.insert(
            Value::String("about_users".into()),
            Value::Sequence(
                self.about_users
                    .iter()
                    .map(|id| Value::Number((*id).into()))
                    .collect(),
            ),
        );
        mapping.insert(
            Value::String("lastUsed".into()),
            Value::String(
                self.last_used
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_else(|| "never".to_owned()),
            ),
        );
        mapping.insert(
            Value::String("usageCount".into()),
            Value::Number(self.usage_count.into()),
        );
        mapping.insert(
            Value::String("embedding".into()),
            Value::Sequence(
                self.embedding
                    .iter()
                    .map(|v| Value::Number((*v as f64).into()))
                    .collect(),
            ),
        );

        let yaml =
            serde_yaml::to_string(&Value::Mapping(mapping)).expect("memory record frontmatter is always representable as yaml");

        format!("---\n{yaml}---\n\n{}\n", self.text)
    }

    pub fn from_markdown(raw: &str) -> Result<Self, MemoryError> {
        let rest = raw
            .strip_prefix("---\n")
            .ok_or_else(|| MemoryError::Format("missing frontmatter start".to_owned()))?;
        let (frontmatter, body) = rest
            .split_once("\n---")
            .ok_or_else(|| MemoryError::Format("missing frontmatter end".to_owned()))?;

        let value: Value = serde_yaml::from_str(frontmatter)?;
        let mapping = value
            .as_mapping()
            .ok_or_else(|| MemoryError::Format("frontmatter is not a mapping".to_owned()))?;

        let get = |key: &str| mapping.get(Value::String(key.to_owned()));

        let id = get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| MemoryError::Format("missing 'id'".to_owned()))?
            .to_owned();
        let confidence = get("confidence").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let visibility = get("visibility")
            .and_then(Value::as_str)
            .map(Visibility::parse)
            .unwrap_or(Visibility::Private);
        let about_users = get("about_users")
            .and_then(Value::as_sequence)
            .map(|seq| seq.iter().filter_map(Value::as_i64).collect())
            .unwrap_or_default();
        let last_used = match get("lastUsed").and_then(Value::as_str) {
            Some("never") | None => None,
            Some(raw) => Some(
                DateTime::parse_from_rfc3339(raw)
                    .map_err(|e| MemoryError::Format(format!("invalid lastUsed: {e}")))?
                    .with_timezone(&Utc),
            ),
        };
        let usage_count = get("usageCount").and_then(Value::as_u64).unwrap_or(0) as u32;
        let embedding = get("embedding")
            .and_then(Value::as_sequence)
            .map(|seq| seq.iter().filter_map(Value::as_f64).map(|v| v as f32).collect())
            .unwrap_or_default();

        Ok(Self {
            id,
            confidence,
            visibility,
            about_users,
            last_used,
            usage_count,
            embedding,
            text: body.trim().to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_all_fields() {
        let record = MemoryRecord::new(
            "У пользователя есть кот по имени Барсик, боится воды.",
            1.0,
            Visibility::Private,
            vec![123456789],
            vec![0.0123, -0.0456, 0.0789],
        );

        let markdown = record.to_markdown();
        let parsed = MemoryRecord::from_markdown(&markdown).expect("valid frontmatter");

        assert_eq!(parsed, record);
    }

    #[test]
    fn never_used_round_trips_as_none() {
        let record = MemoryRecord::new("факт", 0.0, Visibility::Public, vec![], vec![1.0]);
        let markdown = record.to_markdown();
        let parsed = MemoryRecord::from_markdown(&markdown).expect("valid frontmatter");
        assert_eq!(parsed.last_used, None);
    }
}
