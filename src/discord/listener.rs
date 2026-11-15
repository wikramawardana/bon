use crate::discord::commands::{split_message, Data, Error};
use poise::serenity_prelude as serenity;
use tracing::{error, info};

pub async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    data: &Data,
) -> Result<(), Error> {
    match event {
        serenity::FullEvent::Ready { data_about_bot, .. } => {
            info!(
                "💰 Hermes Family Bot is ready as '{}' (ID: {})",
                data_about_bot.user.name, data_about_bot.user.id
            );
        }
        serenity::FullEvent::Message { new_message } => {
            // Ignore bot messages
            if new_message.author.bot {
                return Ok(());
            }

            let bot_id = ctx.cache.current_user().id;
            let is_mentioned = new_message.mentions_user_id(bot_id);
            let is_expense_log_channel =
                new_message.channel_id.get() == data.config.expense_log_channel_id;

            // Process message if it is in the expense log channel or the bot is directly mentioned
            if is_expense_log_channel || is_mentioned {
                let content = new_message.content.trim();
                if content.is_empty() {
                    return Ok(());
                }

                // Strip bot mention if present
                let clean_content = content
                    .replace(&format!("<@{bot_id}>"), "")
                    .replace(&format!("<@!{bot_id}>"), "")
                    .trim()
                    .to_string();

                if clean_content.is_empty() {
                    new_message
                        .reply(
                            &ctx.http,
                            "Hai! Ada yang bisa saya bantu terkait pengeluaran keluarga atau kartu kredit? 💰",
                        )
                        .await?;
                    return Ok(());
                }

                info!(
                    "💬 [FamilyExpenseBot] Processing query from {} in #{}: '{}'",
                    new_message.author.name,
                    new_message.channel_id,
                    clean_content
                );

                // Show typing indicator
                let _ = new_message.channel_id.broadcast_typing(&ctx.http).await;

                match data
                    .agent
                    .chat(new_message.channel_id.get(), &clean_content)
                    .await
                {
                    Ok(reply) => {
                        let chunks = split_message(&reply, 1900);
                        for (i, chunk) in chunks.iter().enumerate() {
                            if i == 0 {
                                new_message.reply(&ctx.http, *chunk).await?;
                            } else {
                                new_message.channel_id.say(&ctx.http, *chunk).await?;
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to generate agent response: {e}");
                        new_message
                            .reply(
                                &ctx.http,
                                format!("❌ Terjadi kesalahan saat memproses permintaan: `{e}`"),
                            )
                            .await?;
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}
