use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub discord_token: String,
    pub openrouter_api_key: String,
    pub llm_base_url: String,
    pub llm_model: String,
    pub family_mcp_url: String,
    pub mcp_token: String,
    pub expense_log_channel_id: u64,
    pub bank_sync_channel_id: u64,
    pub port: u16,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let _ = dotenvy::dotenv();

        let discord_token = env::var("DISCORD_BOT_TOKEN")
            .map_err(|_| anyhow::anyhow!("DISCORD_BOT_TOKEN must be set"))?;

        let openrouter_api_key = env::var("OPENROUTER_API_KEY")
            .unwrap_or_default();

        let llm_base_url = env::var("LLM_BASE_URL")
            .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string());

        let llm_model = env::var("LLM_MODEL")
            .unwrap_or_else(|_| "qwen/qwen-plus".to_string());

        let family_mcp_url = env::var("FAMILY_MCP_URL")
            .unwrap_or_else(|_| "http://family-mcp.family-expense.svc.cluster.local:80/mcp".to_string());

        let mcp_token = env::var("MCP_TOKEN")
            .unwrap_or_default();

        let expense_log_channel_id = env::var("EXPENSE_LOG_CHANNEL_ID")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1571354993850581134);

        let bank_sync_channel_id = env::var("BANK_SYNC_CHANNEL_ID")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1571354991812157440);

        let port = env::var("PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8080);

        Ok(Self {
            discord_token,
            openrouter_api_key,
            llm_base_url,
            llm_model,
            family_mcp_url,
            mcp_token,
            expense_log_channel_id,
            bank_sync_channel_id,
            port,
        })
    }
}
