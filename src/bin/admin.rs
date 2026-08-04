use std::{env, fs, sync::Arc};

use ai_chat_person::{
    adapters::{local_file_storage::LocalFileStorage, timeweb_client::TimewebClient},
    buffer::BufferStore,
    consolidation, memory,
    memory::MemoryStore,
    settings::Settings,
};
use clap::{Parser, Subcommand};
use tokio::sync::RwLock;

/// Ручные операции обслуживания — те же, что фоновые воркеры делают по расписанию,
/// но по требованию и сразу, без ожидания порога/таймера. Telegram не трогает —
/// работает напрямую с буфером/дневником на диске.
#[derive(Parser)]
#[command(name = "admin")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Извлечь факты из буфера переписки в дневник прямо сейчас (не дожидаясь
    /// token_threshold/idle_extraction_after_minutes).
    Extract {
        /// Извлечь только из этого чата (по умолчанию — из всех известных чатов).
        #[arg(long)]
        chat_id: Option<i64>,
    },
    /// Прогнать ночную консолидацию (слияние/удаление фактов + пересборка insights)
    /// прямо сейчас, не дожидаясь 3:00 по локальному времени.
    Sleep,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::from_path_override(".env");

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    let cli = Cli::parse();
    let settings = Settings::load()?;

    let timeweb_token = env::var("TIMEWEB_KEY")?;
    let llm = TimewebClient::try_new(&timeweb_token)?;

    let buffer_storage = LocalFileStorage::new(settings.personality.working_memory_path());
    let buffer = BufferStore::new(buffer_storage).await?;
    let memory = MemoryStore::new(settings.personality.diary_dir_path());

    let model = settings.llm.model;
    let embedding_model = settings.llm.embedding_model;

    match cli.command {
        Command::Extract { chat_id } => {
            let chat_ids = match chat_id {
                Some(id) => vec![id],
                None => buffer.chat_ids().await,
            };

            if chat_ids.is_empty() {
                println!("Нет известных чатов — буфер пуст.");
                return Ok(());
            }

            for chat_id in chat_ids {
                println!("Извлекаю чат {chat_id}...");
                memory::maybe_extract(
                    &llm,
                    &memory,
                    &buffer,
                    chat_id,
                    &settings.memory,
                    &model,
                    &embedding_model,
                )
                .await?;
            }

            buffer.flush().await?;
            println!("Готово.");
        }
        Command::Sleep => {
            let initial_insights =
                fs::read_to_string(settings.personality.insights_path()).unwrap_or_default();
            let insights: consolidation::SharedInsights =
                Arc::new(RwLock::new(Arc::from(initial_insights)));

            println!("Запускаю консолидацию...");
            consolidation::run(
                &llm,
                &memory,
                &settings.memory,
                &model,
                &embedding_model,
                &settings.personality,
                &insights,
            )
            .await?;
            println!("Готово.");
        }
    }

    Ok(())
}
