use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ProactiveSettings {
    pub interval_minutes: u64,
    pub probability: f32,
    pub min_inactivity_minutes: i64,
}
