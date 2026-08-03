use std::{env, fs, sync::Arc};

use ai_chat_person::{
    adapters::{local_file_storage::LocalFileStorage, timeweb_client::TimewebClient},
    bot::ChatBot,
    buffer::BufferStore,
    consolidation,
    memory::MemoryStore,
    proactive,
    settings::Settings,
};
use futures::StreamExt;
use teloxide::{
    Bot,
    requests::Requester,
    types::{AllowedUpdate, UpdateKind},
    update_listeners::{AsUpdateStream, Polling},
};
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::from_path_override(".env");

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    let settings = Settings::load()?;

    tracing::info!(
        diary_dir = %settings.personality.diary_dir_path().display(),
        search_similarity_threshold = settings.memory.search_similarity_threshold,
        dedup_similarity_threshold = settings.memory.dedup_similarity_threshold,
        token_threshold = settings.memory.token_threshold,
        "effective config loaded",
    );

    let bot_token = env::var("BOT_TOKEN")?;
    let bot: Bot = Bot::new(bot_token);
    let bot_user_id = bot.get_me().await?.id.0 as i64;

    let timeweb_token = env::var("TIMEWEB_KEY")?;
    let timeweb_client = TimewebClient::try_new(&timeweb_token)?;

    let buffer_storage = LocalFileStorage::new(settings.personality.working_memory_path());
    let buffer = BufferStore::new(buffer_storage).await?;

    let system_prompt = fs::read_to_string(settings.personality.system_prompt_path())?;
    let memory = MemoryStore::new(settings.personality.diary_dir_path());

    let initial_insights = fs::read_to_string(settings.personality.insights_path()).unwrap_or_default();
    let insights: consolidation::SharedInsights =
        Arc::new(RwLock::new(Arc::from(initial_insights)));

    let model: Arc<str> = settings.llm.model.into();
    let embedding_model: Arc<str> = settings.llm.embedding_model.into();

    consolidation::spawn_daily_task(
        timeweb_client.clone(),
        memory.clone(),
        settings.memory.clone(),
        model.clone(),
        embedding_model.clone(),
        settings.personality.clone(),
        insights.clone(),
    );

    let chat_bot = ChatBot::new(
        bot.clone(),
        bot_user_id,
        timeweb_client,
        buffer.clone(),
        memory,
        settings.memory,
        model,
        embedding_model,
        system_prompt,
        insights,
    );

    proactive::spawn_task(chat_bot.clone(), buffer.clone(), settings.proactive);

    tracing::info!("Starting bot");

    let mut listener = Polling::builder(bot.clone())
        .allowed_updates(vec![
            AllowedUpdate::Message,
            AllowedUpdate::EditedMessage,
            AllowedUpdate::MessageReaction,
        ])
        .build();
    let stream = listener.as_stream();
    tokio::pin!(stream);

    let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());
    loop {
        tokio::select! {
            update = stream.next() => {
                let Some(update) = update else { break };
                let update = match update {
                    Ok(u) => u,
                    Err(err) => {
                        tracing::error!(%err, "polling error");
                        continue;
                    }
                };

                let chat_bot = chat_bot.clone();
                tokio::spawn(async move {
                    let result = match update.kind {
                        UpdateKind::Message(msg) => chat_bot.handle_message(msg).await,
                        UpdateKind::EditedMessage(msg) => chat_bot.handle_edited_message(msg).await,
                        UpdateKind::MessageReaction(reaction) => chat_bot.handle_reaction(reaction).await,
                        _ => Ok(()),
                    };
                    if let Err(err) = result {
                        tracing::error!(%err, "Error handling update");
                    }
                });
            }
            _ = &mut ctrl_c => {
                tracing::info!("Ctrl+C received, shutting down");
                break;
            }
        }
    }

    if let Err(err) = buffer.flush().await {
        tracing::error!(%err, "Can`t flush chat buffer on shutdown");
    }

    Ok(())
}
