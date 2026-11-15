mod agent;
mod config;
mod discord;
mod mcp;

use std::sync::Arc;
use poise::serenity_prelude as serenity;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::agent::FamilyExpenseAgent;
use crate::config::Config;
use crate::discord::commands::{ask, clear, status, summary, transaksi, Data};
use crate::discord::listener::event_handler;
use crate::mcp::McpClient;

async fn health_check() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "healthy",
        "service": "bon",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 0. Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,bon=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("🧾 Starting Bon - Catat Pengeluaran & Bank Sync Bot (Rust)...");

    // 1. Load configuration
    let config = Config::from_env()?;
    let discord_token = config.discord_token.clone();

    // 2. Initialize MCP Client
    let mcp_client = Arc::new(McpClient::new(
        config.family_mcp_url.clone(),
        config.mcp_token.clone(),
    ));

    // 3. Initialize Agent
    let agent = FamilyExpenseAgent::new(config.clone(), mcp_client.clone());

    // 4. Build Poise Discord Framework
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![summary(), transaksi(), ask(), status(), clear()],
            event_handler: |ctx, event, framework, data| {
                Box::pin(event_handler(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                info!(
                    "🧾 Bon logged in as '{}' across {} guilds",
                    _ready.user.name,
                    _ready.guilds.len()
                );
                ctx.set_activity(Some(serenity::ActivityData::watching("over Bon & Pengeluaran 🧾")));

                for guild in &_ready.guilds {
                    let _ = poise::builtins::register_in_guild(ctx, &framework.options().commands, guild.id).await;
                }
                info!("✅ Slash commands registered in guilds!");

                // Start HTTP Server for K8s health checks
                let app = axum::Router::new()
                    .route("/healthz", axum::routing::get(health_check))
                    .route("/health", axum::routing::get(health_check));

                let port = config.port;
                tokio::spawn(async move {
                    let addr = format!("0.0.0.0:{port}");
                    match tokio::net::TcpListener::bind(&addr).await {
                        Ok(listener) => {
                            info!("🌐 Health server listening on http://{addr}");
                            let _ = axum::serve(listener, app).await;
                        }
                        Err(e) => {
                            tracing::error!("Failed to bind health server: {e}");
                        }
                    }
                });

                Ok(Data {
                    agent,
                    config,
                    mcp: mcp_client,
                })
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(
        discord_token,
        serenity::GatewayIntents::non_privileged()
            | serenity::GatewayIntents::MESSAGE_CONTENT
            | serenity::GatewayIntents::GUILD_MESSAGES,
    )
    .framework(framework)
    .await?;

    client.start_autosharded().await?;
    Ok(())
}
