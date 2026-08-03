use std::env;
use std::path::{Path, PathBuf};

use config::{Config, ConfigError};
use serde::Deserialize;

const CONFIG_PATH_ENV: &str = "CONFIG_PATH";
const DEFAULT_CONFIG_PATH: &str = "config.toml";

#[derive(Debug, Clone, Deserialize)]
pub struct PersonalityFiles {
    pub system_prompt: String,
    pub working_memory: String,
    pub diary_dir: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PersonalitySettings {
    pub path: String,
    pub files: PersonalityFiles,
}

impl PersonalitySettings {
    pub fn system_prompt_path(&self) -> PathBuf {
        Path::new(&self.path).join(&self.files.system_prompt)
    }

    pub fn working_memory_path(&self) -> PathBuf {
        Path::new(&self.path).join(&self.files.working_memory)
    }

    pub fn diary_dir_path(&self) -> PathBuf {
        Path::new(&self.path).join(&self.files.diary_dir)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmSettings {
    pub model: String,
    pub embedding_model: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemorySettings {
    pub token_threshold: u32,
    pub dedup_similarity_threshold: f32,
    pub search_similarity_threshold: f32,
    pub search_result_limit: usize,
    pub min_fact_length: usize,
    pub keep_last_messages: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub personality: PersonalitySettings,
    pub memory: MemorySettings,
    pub llm: LlmSettings,
}

impl Settings {
    pub fn load() -> Result<Self, ConfigError> {
        let path = env::var(CONFIG_PATH_ENV).unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());
        Config::builder()
            .add_source(config::File::from(Path::new(&path)))
            .build()?
            .try_deserialize()
    }
}
