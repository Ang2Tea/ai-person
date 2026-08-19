mod settings;

pub use settings::{LlmSettings, Settings};

use std::env;
use std::path::Path;
use std::sync::Arc;

use channel_telegram_bot::history::BufferStore;
use contracts::Storage;
use llm_timeweb::TimewebClient;
use memory::{CommitmentsStore, MemoryStore, PersonalityMemory, SharedInsights};
use storage_fs::FileStorage;
use tokio::sync::RwLock;

const CONFIG_PATH_ENV: &str = "CONFIG_PATH";
const DEFAULT_CONFIG_PATH: &str = "config.toml";

/// bot-core знает только форму `Settings`, не то, откуда они берутся —
/// загрузка (файл, env, что угодно ещё) целиком забота вызывающего
/// бинарника. Env (`APP__...`) поверх файла — второй, необязательный
/// источник, чтобы в контейнере/CI можно было переопределить отдельные
/// поля без перезаписи всего файла (`.required(false)`, т.к. в некоторых
/// окружениях конфиг целиком может задаваться только через env).
pub fn load_settings() -> Result<Settings, config::ConfigError> {
    let path = env::var(CONFIG_PATH_ENV).unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());
    config::Config::builder()
        .add_source(config::File::from(Path::new(&path)).required(false))
        .add_source(
            config::Environment::with_prefix("APP")
                .try_parsing(true)
                .separator("__"),
        )
        .build()?
        .try_deserialize()
}

/// Общая для `bot`/`admin` инициализация — оба бинарника поднимают один и
/// тот же набор зависимостей (логирование, LLM-клиент, история/память),
/// только по-разному их используют дальше.
pub fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        // Логирует закрытие каждого span'а (`#[instrument]` и явные
        // `info_span!`) с `time.busy`/`time.idle` — без этого спаны дают
        // только корреляцию полей во вложенных событиях, но не видно, сколько
        // реально занял ход, вызов LLM или фоновая задача.
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
        .init();
}

pub fn init_llm(settings: &Settings) -> Result<TimewebClient, Box<dyn std::error::Error>> {
    let timeweb_token = env::var("TIMEWEB_KEY")?;
    Ok(TimewebClient::try_new(
        &timeweb_token,
        settings.llm.model.clone(),
        settings.llm.embedding_model.clone(),
        settings.llm.vision_model.clone(),
    )?)
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

/// История переписки (канал) — забота `channel-telegram-bot`, `app` только
/// собирает конкретный `Storage`-backend для неё.
pub async fn init_history(settings: &Settings) -> Result<BufferStore<FileStorage>, Box<dyn std::error::Error>> {
    let storage = personality_storage(settings);
    let history = BufferStore::new(storage, settings.personality.files.working_memory.clone()).await?;
    Ok(history)
}

/// Долгосрочная память личности (дневник + commitments + insights), собранная
/// за `contracts::Memory` — `app` единственный, кто видит конкретный тип
/// `PersonalityMemory<TimewebClient, FileStorage>`.
pub async fn init_memory(
    settings: &Settings,
    llm: TimewebClient,
) -> Result<PersonalityMemory<TimewebClient, FileStorage>, Box<dyn std::error::Error>> {
    let store = MemoryStore::new(FileStorage::new(&personality_subdir(
        settings,
        &settings.personality.files.diary_dir,
    )));
    let commitments = CommitmentsStore::new(
        personality_storage(settings),
        settings.personality.files.commitments.clone(),
    );

    let system_prompt = read_system_prompt(settings).await?;
    let initial_insights = read_insights(settings).await;
    let insights: SharedInsights = Arc::new(RwLock::new(Arc::from(initial_insights)));

    Ok(PersonalityMemory::new(
        llm,
        store,
        commitments,
        insights,
        system_prompt,
        settings.memory.clone(),
        personality_storage(settings),
        settings.personality.files.system_prompt.clone(),
        settings.personality.files.insights.clone(),
    ))
}

/// Отдельный `FileStorage`, рядом с которым живут `system_prompt.md`/
/// `insights.md` — нужен и здесь (для `read_system_prompt`/`read_insights`),
/// и `init_memory` (`PersonalityMemory::consolidate` пишет туда же).
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
