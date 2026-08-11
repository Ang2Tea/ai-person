use app::{init_history, init_llm, init_memory, init_tracing, load_settings};
use clap::{Parser, Subcommand};
use contracts::Memory;

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
    let history = init_history(&settings).await?;
    let memory = init_memory(&settings, llm).await?;

    match cli.command {
        Command::Extract { chat_id } => {
            let chat_ids = match chat_id {
                Some(id) => vec![id],
                None => history.chat_ids().await,
            };

            if chat_ids.is_empty() {
                println!("Нет известных чатов — буфер пуст.");
                return Ok(());
            }

            for chat_id in chat_ids {
                println!("Извлекаю чат {chat_id}...");
                let Some(chat_buffer) = history.get(chat_id).await else {
                    continue;
                };
                memory.extract(chat_id, &chat_buffer.to_transcript()).await;
                history
                    .truncate_keep_last(chat_id, memory.keep_last_messages())
                    .await;
            }

            history.flush().await?;
            println!("Готово.");
        }
        Command::Sleep => {
            println!("Запускаю консолидацию...");
            memory.consolidate().await?;
            println!("Готово.");
        }
    }

    Ok(())
}
