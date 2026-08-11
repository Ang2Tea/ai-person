use chrono::Utc;
use teloxide::{
    Bot,
    dispatching::dialogue::GetChatId,
    requests::Requester,
    types::{ChatId, MaybeAnonymousUser, Message, MessageReactionUpdated, ReactionType, UpdateKind},
};

use bot_core::bot::ChatBot;
use contracts::{ChannelId, Llm, Memory, Storage};

use crate::errors::DispatchError;
use crate::history::{BufferStore, BufferedMessage};

const TELEGRAM_CHANNEL: &str = "telegram";

/// Единая точка входа для апдейтов поллинга — разбирает `UpdateKind` и
/// перенаправляет в конкретный обработчик; остальные виды апдейтов (не
/// запрошены `AllowedUpdate` при подписке) молча игнорируются.
pub async fn handle_update<L, M, S>(
    bot: &Bot,
    bot_user_id: i64,
    chat_bot: &ChatBot<L, M>,
    history: &BufferStore<S>,
    kind: UpdateKind,
) -> Result<(), DispatchError>
where
    L: Llm + Clone + Send + Sync + 'static,
    M: Memory + Clone + Send + Sync + 'static,
    S: Storage + Clone + Send + Sync + 'static,
{
    match kind {
        UpdateKind::Message(msg) => handle_message(bot, bot_user_id, chat_bot, history, msg).await,
        UpdateKind::EditedMessage(msg) => {
            handle_edited_message(bot, bot_user_id, chat_bot, history, msg).await
        }
        UpdateKind::MessageReaction(reaction) => {
            handle_reaction(bot, bot_user_id, chat_bot, history, reaction).await
        }
        _ => Ok(()),
    }
}

async fn handle_message<L, M, S>(
    bot: &Bot,
    bot_user_id: i64,
    chat_bot: &ChatBot<L, M>,
    history: &BufferStore<S>,
    msg: Message,
) -> Result<(), DispatchError>
where
    L: Llm + Clone + Send + Sync + 'static,
    M: Memory + Clone + Send + Sync + 'static,
    S: Storage + Clone + Send + Sync + 'static,
{
    let Some(chat_id) = msg.chat_id() else {
        tracing::error!("Can`t get chat id");
        return Ok(());
    };

    let Some(text) = describe_message(&msg) else {
        tracing::debug!("Skipping message without representable content");
        return Ok(());
    };

    let Some(from) = &msg.from else {
        tracing::error!("Can`t get message sender");
        return Ok(());
    };

    let incoming = BufferedMessage {
        telegram_message_id: msg.id.0,
        sender_id: ChatId::from(from.id).0,
        sender_name: from.first_name.clone(),
        text: text.clone(),
        timestamp: Utc::now(),
        is_bot: false,
    };
    history.push(chat_id.0, incoming).await;

    run_and_reply(bot, bot_user_id, chat_bot, history, chat_id, from.id.0 as i64, &text).await
}

/// Правки в Telegram применяются только к тексту/подписи — dice, стикер,
/// опрос и т.п. отредактировать в другой тип контента нельзя, поэтому здесь
/// достаточно текста/подписи, в отличие от `describe_message`.
async fn handle_edited_message<L, M, S>(
    bot: &Bot,
    bot_user_id: i64,
    chat_bot: &ChatBot<L, M>,
    history: &BufferStore<S>,
    msg: Message,
) -> Result<(), DispatchError>
where
    L: Llm + Clone + Send + Sync + 'static,
    M: Memory + Clone + Send + Sync + 'static,
    S: Storage + Clone + Send + Sync + 'static,
{
    let Some(chat_id) = msg.chat_id() else {
        tracing::error!("Can`t get chat id for edited message");
        return Ok(());
    };

    let Some(new_text) = msg.text().or_else(|| msg.caption()) else {
        tracing::debug!("Skipping edited message without text/caption");
        return Ok(());
    };

    let Some(from) = &msg.from else {
        tracing::error!("Can`t get edited message sender");
        return Ok(());
    };

    let text = format!(
        "{} отредактировал(а) сообщение #{}, теперь: {}",
        from.first_name, msg.id.0, new_text
    );

    let incoming = BufferedMessage {
        telegram_message_id: msg.id.0,
        sender_id: ChatId::from(from.id).0,
        sender_name: from.first_name.clone(),
        text: text.clone(),
        timestamp: Utc::now(),
        is_bot: false,
    };
    history.push(chat_id.0, incoming).await;

    run_and_reply(bot, bot_user_id, chat_bot, history, chat_id, from.id.0 as i64, &text).await
}

async fn handle_reaction<L, M, S>(
    bot: &Bot,
    bot_user_id: i64,
    chat_bot: &ChatBot<L, M>,
    history: &BufferStore<S>,
    reaction: MessageReactionUpdated,
) -> Result<(), DispatchError>
where
    L: Llm + Clone + Send + Sync + 'static,
    M: Memory + Clone + Send + Sync + 'static,
    S: Storage + Clone + Send + Sync + 'static,
{
    let MaybeAnonymousUser::User(user) = &reaction.actor else {
        tracing::debug!("Skipping reaction from an anonymous/channel actor");
        return Ok(());
    };

    if user.id.0 as i64 == bot_user_id {
        // Не реагируем на собственные же реакции — иначе потенциальный цикл.
        return Ok(());
    }

    if reaction.new_reaction.is_empty() {
        // Реакцию сняли, а не поставили — этот случай не описан заданием.
        return Ok(());
    }

    let chat_id = reaction.chat.id;
    let emoji = describe_reactions(&reaction.new_reaction);
    let text = format!(
        "{} поставил(а) реакцию {} на сообщение #{}",
        user.first_name, emoji, reaction.message_id.0
    );

    let incoming = BufferedMessage {
        telegram_message_id: reaction.message_id.0,
        sender_id: user.id.0 as i64,
        sender_name: user.first_name.clone(),
        text: text.clone(),
        timestamp: reaction.date,
        is_bot: false,
    };
    history.push(chat_id.0, incoming).await;

    run_and_reply(bot, bot_user_id, chat_bot, history, chat_id, user.id.0 as i64, &text).await
}

async fn run_and_reply<L, M, S>(
    bot: &Bot,
    bot_user_id: i64,
    chat_bot: &ChatBot<L, M>,
    history: &BufferStore<S>,
    chat_id: ChatId,
    user_id: i64,
    query: &str,
) -> Result<(), DispatchError>
where
    L: Llm + Clone + Send + Sync + 'static,
    M: Memory + Clone + Send + Sync + 'static,
    S: Storage + Clone + Send + Sync + 'static,
{
    let chat = ChannelId {
        channel: TELEGRAM_CHANNEL,
        id: chat_id.0.to_string(),
    };
    let user = ChannelId {
        channel: TELEGRAM_CHANNEL,
        id: user_id.to_string(),
    };

    let Some(text) = chat_bot.run_turn(chat, user, query).await? else {
        return Ok(());
    };

    send_and_record(bot, bot_user_id, history, chat_id, text).await
}

pub(crate) async fn send_and_record<S>(
    bot: &Bot,
    bot_user_id: i64,
    history: &BufferStore<S>,
    chat_id: ChatId,
    text: String,
) -> Result<(), DispatchError>
where
    S: Storage + Clone + Send + Sync + 'static,
{
    let sent = bot.send_message(chat_id, &text).await?;

    let outgoing = BufferedMessage {
        telegram_message_id: sent.id.0,
        sender_id: bot_user_id,
        sender_name: "bot".to_owned(),
        text,
        timestamp: Utc::now(),
        is_bot: true,
    };
    history.push(chat_id.0, outgoing).await;

    Ok(())
}

/// Текстовое описание содержимого сообщения — вместо голого `.text()`, чтобы
/// опросы/dice/геолокация/контакты/стикеры/подписи тоже попадали в буфер и
/// проходили через общий tool-calling цикл, а не отбрасывались молча. Сама
/// медиа-часть (фото/стикер/голос) не разбирается — это отдельная, более
/// дорогая задача (нужен доп. вызов другой модели).
fn describe_message(msg: &Message) -> Option<String> {
    if let Some(text) = msg.text() {
        return Some(text.to_owned());
    }
    if let Some(caption) = msg.caption() {
        return Some(caption.to_owned());
    }
    if let Some(poll) = msg.poll() {
        let options = poll
            .options
            .iter()
            .map(|o| o.text.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let mut description = format!("запустил опрос: {} — варианты: {}", poll.question, options);
        if let Some(explanation) = &poll.explanation {
            description.push_str(&format!(" (пояснение: {explanation})"));
        }
        return Some(description);
    }
    if let Some(dice) = msg.dice() {
        let emoji = match dice.emoji {
            teloxide::types::DiceEmoji::Dice => "🎲",
            teloxide::types::DiceEmoji::Darts => "🎯",
            teloxide::types::DiceEmoji::Bowling => "🎳",
            teloxide::types::DiceEmoji::Basketball => "🏀",
            teloxide::types::DiceEmoji::Football => "⚽",
            teloxide::types::DiceEmoji::SlotMachine => "🎰",
        };
        return Some(format!("бросил {emoji}, результат: {}", dice.value));
    }
    if let Some(venue) = msg.venue() {
        return Some(format!(
            "поделился гео локацией: {} ({})",
            venue.title, venue.address
        ));
    }
    if msg.location().is_some() {
        return Some("поделился гео-локацией".to_owned());
    }
    if let Some(contact) = msg.contact() {
        let last_name = contact.last_name.clone().unwrap_or_default();
        return Some(
            format!(
                "поделился контактом: {} {}, {}",
                contact.first_name, last_name, contact.phone_number
            )
            .trim()
            .to_owned(),
        );
    }
    if let Some(sticker) = msg.sticker() {
        let emoji = sticker.emoji.clone().unwrap_or_default();
        return Some(format!("отправил стикер {emoji}"));
    }

    None
}

/// Эмодзи, реально поставленные (после изменения) — для текста в буфер.
fn describe_reactions(reactions: &[ReactionType]) -> String {
    reactions
        .iter()
        .map(|r| match r {
            ReactionType::Emoji { emoji } => emoji.clone(),
            ReactionType::CustomEmoji { .. } => "кастомный эмодзи".to_owned(),
            ReactionType::Paid => "⭐".to_owned(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}
