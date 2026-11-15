use std::sync::Arc;
use crate::agent::{format_idr, FamilyExpenseAgent};
use crate::config::Config;
use crate::mcp::McpClient;
use serde_json::Value;

pub struct Data {
    pub agent: FamilyExpenseAgent,
    pub config: Config,
    pub mcp: Arc<McpClient>,
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

#[derive(Debug, poise::ChoiceParameter)]
pub enum GroupByChoice {
    #[name = "Kartu Kredit"]
    Card,
    #[name = "Kategori Pengeluaran"]
    Category,
    #[name = "Penanggung Jawab (Paid By)"]
    PaidBy,
    #[name = "Status Pembayaran"]
    Status,
}

impl GroupByChoice {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Card => "card",
            Self::Category => "category",
            Self::PaidBy => "paid_by",
            Self::Status => "status",
        }
    }
}

#[derive(Debug, poise::ChoiceParameter)]
pub enum CardChoice {
    #[name = "BCA KrisFlyer"]
    BcaKrisFlyer,
    #[name = "Mandiri Marriott Bonvoy"]
    MandiriMarriott,
    #[name = "BNI Mastercard World"]
    BniMastercard,
    #[name = "Jenius Credit Card"]
    Jenius,
}

impl CardChoice {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BcaKrisFlyer => "BCA KrisFlyer",
            Self::MandiriMarriott => "Mandiri Marriott Bonvoy",
            Self::BniMastercard => "BNI Mastercard World",
            Self::Jenius => "Jenius Credit Card",
        }
    }
}

#[derive(Debug, poise::ChoiceParameter)]
pub enum StatusChoice {
    #[name = "PAID (Sudah Lunas)"]
    Paid,
    #[name = "PENDING (Menunggu Rekon)"]
    Pending,
    #[name = "UNPAID (Belum Bayar)"]
    Unpaid,
}

impl StatusChoice {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Paid => "PAID",
            Self::Pending => "PENDING",
            Self::Unpaid => "UNPAID",
        }
    }
}

#[derive(Debug, poise::ChoiceParameter)]
pub enum PaidByChoice {
    #[name = "Wikra"]
    Wikra,
    #[name = "Dapur Bu Wikra"]
    DapurBuWikra,
    #[name = "Mbak Intan"]
    MbakIntan,
    #[name = "Vincent"]
    Vincent,
    #[name = "Kepin"]
    Kepin,
    #[name = "Teteh"]
    Teteh,
}

impl PaidByChoice {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Wikra => "Wikra",
            Self::DapurBuWikra => "Dapur Bu Wikra",
            Self::MbakIntan => "Mbak Intan",
            Self::Vincent => "Vincent",
            Self::Kepin => "Kepin",
            Self::Teteh => "Teteh",
        }
    }
}

pub fn split_message(text: &str, max_len: usize) -> Vec<&str> {
    if text.len() <= max_len {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let remaining = &text[start..];
        if remaining.len() <= max_len {
            chunks.push(remaining);
            break;
        }

        let split_at = if let Some(idx) = remaining[..max_len].rfind("\n\n") {
            idx + 2
        } else if let Some(idx) = remaining[..max_len].rfind('\n') {
            idx + 1
        } else if let Some(idx) = remaining[..max_len].rfind(' ') {
            idx + 1
        } else {
            max_len
        };

        chunks.push(&remaining[..split_at]);
        start += split_at;
    }

    chunks
}

pub async fn send_chunked_reply(ctx: Context<'_>, text: &str) -> Result<(), Error> {
    let chunks = split_message(text, 1900);
    for (i, chunk) in chunks.iter().enumerate() {
        if i == 0 {
            ctx.say(*chunk).await?;
        } else {
            ctx.channel_id().say(&ctx.http(), *chunk).await?;
        }
    }
    Ok(())
}

/// Ringkasan pengeluaran keluarga (total, per kartu, per kategori, atau per orang)
#[poise::command(slash_command)]
pub async fn summary(
    ctx: Context<'_>,
    #[description = "Bulan (contoh: 'November 2026', 'Oktober 2026', kosong = bulan ini)"] month: Option<String>,
    #[description = "Kelompokkan berdasarkan"] group_by: Option<GroupByChoice>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let m = month.as_deref();
    let g = group_by.as_ref().map(|x| x.as_str()).unwrap_or("card");

    match ctx.data().mcp.summarize_transactions(m, Some(g)).await {
        Ok(val) => {
            let tab = val["tab"].as_str().unwrap_or("Bulan Berjalan");
            let total = val["total"].as_i64().unwrap_or(0);
            let count = val["count"].as_i64().unwrap_or(0);

            let mut lines = Vec::new();
            lines.push(format!("📊 **Ringkasan Pengeluaran: `{tab}`**"));
            lines.push(format!("💰 **Total:** `{}` ({} transaksi)", format_idr(total), count));
            lines.push(format!("🏷️ **Dikelompokkan Berdasarkan:** `{}`\n", g));

            if let Some(groups) = val["groups"].as_object() {
                let mut sorted_groups: Vec<(&String, &Value)> = groups.iter().collect();
                sorted_groups.sort_by(|a, b| {
                    let total_a = a.1["total"].as_i64().unwrap_or(0);
                    let total_b = b.1["total"].as_i64().unwrap_or(0);
                    total_b.cmp(&total_a)
                });

                for (name, data) in sorted_groups {
                    let g_total = data["total"].as_i64().unwrap_or(0);
                    let g_count = data["count"].as_i64().unwrap_or(0);
                    lines.push(format!("• **{}**: `{}` ({} tx)", name, format_idr(g_total), g_count));
                }
            }

            lines.push("\n`Bon 🧾`".to_string());
            let report = lines.join("\n");
            send_chunked_reply(ctx, &report).await?;
        }
        Err(e) => {
            ctx.say(format!("❌ Gagal mengambil ringkasan: `{e}`")).await?;
        }
    }
    Ok(())
}

/// Cari dan tampilkan daftar transaksi pengeluaran dari Google Sheet
#[poise::command(slash_command)]
pub async fn transaksi(
    ctx: Context<'_>,
    #[description = "Bulan (contoh: 'November 2026', kosong = bulan ini)"] month: Option<String>,
    #[description = "Kartu spesifik"] card: Option<CardChoice>,
    #[description = "Status pembayaran"] status: Option<StatusChoice>,
    #[description = "Penanggung jawab"] paid_by: Option<PaidByChoice>,
    #[description = "Kata kunci nama transaksi/merchant"] keyword: Option<String>,
    #[description = "Nominal minimum (IDR)"] min_amount: Option<u64>,
) -> Result<(), Error> {
    ctx.defer().await?;

    match ctx.data().mcp.list_transactions(
        month.as_deref(),
        card.as_ref().map(|x| x.as_str()),
        status.as_ref().map(|x| x.as_str()),
        paid_by.as_ref().map(|x| x.as_str()),
        min_amount,
        None,
        keyword.as_deref(),
    ).await {
        Ok(val) => {
            let count = val["count"].as_u64().unwrap_or(0);
            let tab = val["tab"].as_str().unwrap_or("");

            if count == 0 {
                ctx.say(format!("🔍 Tidak ditemukan transaksi yang cocok di tab `{tab}`.\n\n`Hermes Family Bot 🦀`")).await?;
                return Ok(());
            }

            let mut lines = Vec::new();
            lines.push(format!("📋 **Daftar Transaksi ({tab}) — Total {count} Ditemukan:**\n"));

            if let Some(rows) = val["rows"].as_array() {
                let display_limit = 15;
                for (i, r) in rows.iter().take(display_limit).enumerate() {
                    let no = i + 1;
                    let date = r["date"].as_str().unwrap_or("-");
                    let item = r["item"].as_str().unwrap_or("-");
                    let amount = r["amount"].as_i64().unwrap_or(0);
                    let card_name = r["card"].as_str().unwrap_or("-");
                    let r_status = r["status"].as_str().unwrap_or("PENDING");
                    let r_paid = r["paid_by"].as_str().unwrap_or("-");
                    let r_ref = r["ref"].as_str().unwrap_or("");

                    let status_emoji = match r_status {
                        "PAID" => "✅",
                        "PENDING" => "⏳",
                        _ => "❌",
                    };

                    lines.push(format!(
                        "{}. {} **{}** — `{}` ({})\n   └ 💳 {} | 👤 {} | Ref: `{}`",
                        no, status_emoji, item, format_idr(amount), date, card_name, r_paid, r_ref
                    ));
                }

                if count as usize > display_limit {
                    lines.push(format!("\n*(+{} transaksi lainnya disembunyikan)*", count as usize - display_limit));
                }
            }

            lines.push("\n`Bon 🧾`".to_string());
            let report = lines.join("\n");
            send_chunked_reply(ctx, &report).await?;
        }
        Err(e) => {
            ctx.say(format!("❌ Gagal mencari transaksi: `{e}`")).await?;
        }
    }
    Ok(())
}

/// Tanya asisten AI seputar pengeluaran, perbandingan bulan, atau tips hemat
#[poise::command(slash_command)]
pub async fn ask(
    ctx: Context<'_>,
    #[description = "Pertanyaan Anda mengenai keuangan keluarga"] prompt: String,
) -> Result<(), Error> {
    ctx.defer().await?;
    let channel_id = ctx.channel_id().get();
    match ctx.data().agent.chat(channel_id, &prompt).await {
        Ok(ans) => {
            send_chunked_reply(ctx, &ans).await?;
        }
        Err(e) => {
            ctx.say(format!("❌ Terjadi kesalahan: `{e}`")).await?;
        }
    }
    Ok(())
}

/// Periksa status bot dan koneksi FastMCP server
#[poise::command(slash_command)]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let mcp_healthy = ctx.data().mcp.check_health().await;
    let mcp_status_str = if mcp_healthy { "🟢 Connected (OK)" } else { "🔴 Disconnected / Error" };

    let months = ctx.data().mcp.list_months().await.unwrap_or_default();
    let months_str = if months.is_empty() { "-".into() } else { months.join(", ") };

    let report = format!(
        "💰 **Hermes Family Finance Bot Status**\n\
         - **Runtime:** Rust 🦀 Serenity & Poise\n\
         - **LLM Model:** `{}` (OpenRouter)\n\
         - **Family MCP Server:** `{}`\n\
         - **MCP Connection:** {}\n\
         - **Available Sheets Tabs:** `{}`\n\
         - **Expense Log Channel:** <#{}>\n\
         - **Bank Sync Feed Channel:** <#{}>\n\n\
         `Bon 🧾`",
        ctx.data().config.llm_model,
        ctx.data().config.family_mcp_url,
        mcp_status_str,
        months_str,
        ctx.data().config.expense_log_channel_id,
        ctx.data().config.bank_sync_channel_id
    );

    send_chunked_reply(ctx, &report).await?;
    Ok(())
}

/// Bersihkan riwayat percakapan thread/channel saat ini
#[poise::command(slash_command)]
pub async fn clear(ctx: Context<'_>) -> Result<(), Error> {
    let channel_id = ctx.channel_id().get();
    ctx.data().agent.memory().clear(channel_id).await;
    ctx.say("🧹 Riwayat percakapan pengeluaran di channel ini telah dibersihkan!\n\n`Bon 🧾`").await?;
    Ok(())
}
