use std::sync::Arc;

use app::{init_llm, init_storage, init_tracing, load_settings, personality_storage, read_insights};
use bot_core::{consolidation, memory};
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

    init_tracing();

    let cli = Cli::parse();
    let settings = load_settings()?;

    let llm = init_llm()?;
    let (buffer, memory, commitments) = init_storage(&settings).await?;
    let personality_storage = personality_storage(&settings);

    let model = settings.llm.model.clone();
    let embedding_model = settings.llm.embedding_model.clone();

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
                    &commitments,
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
            let initial_insights = read_insights(&settings).await;
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
                &personality_storage,
                &insights,
            )
            .await?;
            println!("Готово.");
        }
    }

    Ok(())
}
