mod settings;

pub use settings::{LlmSettings, Settings};

use std::env;
use std::path::Path;

use bot_core::{buffer::BufferStore, commitments::CommitmentsStore, memory::MemoryStore};
use contracts::Storage;
use llm_timeweb::TimewebClient;
use storage_fs::FileStorage;

const CONFIG_PATH_ENV: &str = "CONFIG_PATH";
const DEFAULT_CONFIG_PATH: &str = "config.toml";

/// bot-core знает только форму `Settings`, не то, откуда они берутся —
/// загрузка (файл, env, что угодно ещё) целиком забота вызывающего
/// бинарника.
pub fn load_settings() -> Result<Settings, config::ConfigError> {
    let path = env::var(CONFIG_PATH_ENV).unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());
    config::Config::builder()
        .add_source(config::File::from(Path::new(&path)))
        .build()?
        .try_deserialize()
}

/// Общая для `bot`/`admin` инициализация — оба бинарника поднимают один и
/// тот же набор зависимостей (логирование, LLM-клиент, буфер/дневник/
/// commitments), только по-разному их используют дальше.
pub fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();
}

pub fn init_llm() -> Result<TimewebClient, Box<dyn std::error::Error>> {
    let timeweb_token = env::var("TIMEWEB_KEY")?;
    Ok(TimewebClient::try_new(&timeweb_token)?)
}

/// `personality.path` + подпапка — `PersonalitySettings` больше не даёт таких
/// методов сама (не её дело знать про файловые пути), это чисто локальная
/// склейка ключа для `FileStorage::new`.
fn personality_subdir(settings: &Settings, relative: &str) -> String {
    Path::new(&settings.personality.path)
        .join(relative)
        .to_string_lossy()
        .into_owned()
}

pub async fn init_storage(
    settings: &Settings,
) -> Result<
    (
        BufferStore<FileStorage>,
        MemoryStore<FileStorage>,
        CommitmentsStore<FileStorage>,
    ),
    Box<dyn std::error::Error>,
> {
    let buffer_storage = FileStorage::new(&settings.personality.path);
    let buffer = BufferStore::new(
        buffer_storage,
        settings.personality.files.working_memory.clone(),
    )
    .await?;

    let memory = MemoryStore::new(FileStorage::new(&personality_subdir(
        settings,
        &settings.personality.files.diary_dir,
    )));
    let commitments = CommitmentsStore::new(FileStorage::new(&personality_subdir(
        settings,
        &settings.personality.files.commitments_dir,
    )));

    Ok((buffer, memory, commitments))
}

/// Отдельный `FileStorage`, рядом с которым живут `system_prompt.md`/
/// `insights.md` — нужен и здесь (для `read_system_prompt`/`read_insights`),
/// и вызывающему коду для `consolidation::spawn_daily_task`/`run`
/// (`write_insights` пишет туда же).
pub fn personality_storage(settings: &Settings) -> FileStorage {
    FileStorage::new(&settings.personality.path)
}

/// Читают файлы личности через `Storage`, а не напрямую `std::fs` — чтобы
/// поменять backend (например на S3) позже нужно было только поменять
/// реализацию, отдаваемую `FileStorage::new`, не переписывать вызывающий код.
pub async fn read_system_prompt(settings: &Settings) -> Result<String, contracts::StorageError> {
    personality_storage(settings)
        .get(settings.personality.files.system_prompt.clone())
        .await
}

/// Отсутствие `insights.md` — нормальный случай (личность ещё ни разу не
/// консолидировалась), не ошибка запуска, поэтому `unwrap_or_default`, а не `?`.
pub async fn read_insights(settings: &Settings) -> String {
    personality_storage(settings)
        .get(settings.personality.files.insights.clone())
        .await
        .unwrap_or_default()
}
