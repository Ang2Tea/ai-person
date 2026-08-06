use std::{env, sync::Arc};

use app::{
    init_llm, init_storage, init_tracing, load_settings, personality_storage, read_insights,
    read_system_prompt,
};
use bot_core::{bot::ChatBot, consolidation, idle_extraction, proactive};
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

    init_tracing();

    let settings = load_settings()?;

    tracing::info!(
        diary_dir = %settings.personality.files.diary_dir,
        auto_retrieval_similarity_threshold = settings.memory.auto_retrieval_similarity_threshold,
        dedup_similarity_threshold = settings.memory.dedup_similarity_threshold,
        token_threshold = settings.memory.token_threshold,
        "effective config loaded",
    );

    let bot_token = env::var("BOT_TOKEN")?;
    let bot: Bot = Bot::new(bot_token);
    let bot_user_id = bot.get_me().await?.id.0 as i64;

    let timeweb_client = init_llm()?;
    let (buffer, memory, commitments) = init_storage(&settings).await?;
    let personality_storage = personality_storage(&settings);

    let system_prompt = read_system_prompt(&settings).await?;

    let initial_insights = read_insights(&settings).await;
    let insights: consolidation::SharedInsights =
        Arc::new(RwLock::new(Arc::from(initial_insights)));

    let model: Arc<str> = settings.llm.model.into();
    let embedding_model: Arc<str> = settings.llm.embedding_model.into();

    let mut tools = bot_core::tools::tools(
        buffer.clone(),
        timeweb_client.clone(),
        memory.clone(),
        settings.memory.clone(),
        embedding_model.clone(),
    );
    tools.extend(channel_telegram_bot::tools(
        bot.clone(),
        bot_user_id,
        buffer.clone(),
    ));

    consolidation::spawn_daily_task(
        timeweb_client.clone(),
        memory.clone(),
        settings.memory.clone(),
        model.clone(),
        embedding_model.clone(),
        settings.personality.clone(),
        personality_storage,
        insights.clone(),
    );

    idle_extraction::spawn_task(
        timeweb_client.clone(),
        memory.clone(),
        buffer.clone(),
        commitments.clone(),
        settings.memory.clone(),
        model.clone(),
        embedding_model.clone(),
    );

    let chat_bot = ChatBot::new(
        bot.clone(),
        bot_user_id,
        timeweb_client,
        buffer.clone(),
        memory,
        settings.memory,
        commitments,
        model,
        embedding_model,
        system_prompt,
        insights,
        tools,
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
