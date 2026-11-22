use crate::config::Config;
use crate::mcp::McpClient;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error};

pub fn format_idr(amount: i64) -> String {
    let is_negative = amount < 0;
    let abs_amount = amount.abs();
    let s = abs_amount.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();

    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            result.push('.');
        }
        result.push(*c);
    }

    if is_negative {
        format!("-Rp {}", result)
    } else {
        format!("Rp {}", result)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Default)]
pub struct MemoryStore {
    // channel_id -> message history
    history: Arc<RwLock<HashMap<u64, Vec<ChatMessage>>>>,
}

impl MemoryStore {
    pub async fn get_history(&self, channel_id: u64) -> Vec<ChatMessage> {
        let read = self.history.read().await;
        read.get(&channel_id).cloned().unwrap_or_default()
    }

    pub async fn append_message(&self, channel_id: u64, message: ChatMessage) {
        let mut write = self.history.write().await;
        let list = write.entry(channel_id).or_default();
        list.push(message);
        // Keep last 16 turns to avoid context blowout
        if list.len() > 16 {
            let start = list.len() - 16;
            *list = list[start..].to_vec();
        }
    }

    pub async fn clear(&self, channel_id: u64) {
        let mut write = self.history.write().await;
        write.remove(&channel_id);
    }
}

#[derive(Clone)]
pub struct FamilyExpenseAgent {
    config: Config,
    mcp: Arc<McpClient>,
    memory: MemoryStore,
    http: reqwest::Client,
}

impl FamilyExpenseAgent {
    pub fn new(config: Config, mcp: Arc<McpClient>) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", config.openrouter_api_key)) {
            headers.insert(AUTHORIZATION, val);
        }

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_default();

        Self {
            config,
            mcp,
            memory: MemoryStore::default(),
            http,
        }
    }

    pub fn memory(&self) -> &MemoryStore {
        &self.memory
    }

    #[allow(dead_code)]
    pub fn mcp(&self) -> Arc<McpClient> {
        self.mcp.clone()
    }

    fn tools_spec(&self) -> Vec<Value> {
        vec![
            json!({
                "type": "function",
                "function": {
                    "name": "list_months",
                    "description": "Daftar bulan / tab sheet pengeluaran keluarga yang tersedia (contoh: 'Oktober 2026', 'November 2026').",
                    "parameters": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                }
            }),
            json!({
                "type": "function",
                "function": {
                    "name": "summarize_transactions",
                    "description": "Hitung total dan ringkasan pengeluaran bulan tertentu, bisa dikelompokkan berdasarkan kartu, kategori, penanggung jawab (paid_by), atau status pembayaran.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "month": {
                                "type": "string",
                                "description": "Nama bulan, e.g. 'November 2026', 'Oktober 2026', '2026-11', atau kosongkan untuk bulan berjalan saat ini."
                            },
                            "group_by": {
                                "type": "string",
                                "enum": ["card", "category", "paid_by", "status"],
                                "description": "Kategori pengelompokan (default: 'card')."
                            }
                        },
                        "required": []
                    }
                }
            }),
            json!({
                "type": "function",
                "function": {
                    "name": "list_transactions",
                    "description": "Cari dan tampilkan daftar transaksi pengeluaran dari sheet dengan filter lengkap.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "month": {
                                "type": "string",
                                "description": "Nama bulan (contoh: 'November 2026', '2026-11'). Kosongkan untuk bulan saat ini."
                            },
                            "card": {
                                "type": "string",
                                "enum": ["BCA KrisFlyer", "Mandiri Marriott Bonvoy", "BNI Mastercard World", "Jenius Credit Card"],
                                "description": "Filter kartu spesifik."
                            },
                            "status": {
                                "type": "string",
                                "enum": ["PAID", "PENDING", "UNPAID"],
                                "description": "Filter status transaksi."
                            },
                            "paid_by": {
                                "type": "string",
                                "enum": ["Wikra", "Dapur Bu Wikra", "Mbak Intan", "Vincent", "Kepin", "Teteh"],
                                "description": "Filter siapa yang membayar."
                            },
                            "category": {
                                "type": "string",
                                "description": "Filter kategori pengeluaran spesifik."
                            },
                            "note_contains": {
                                "type": "string",
                                "description": "Kata kunci pencarian pada kolom catatan/keterangan/notes transaksi (contoh: 'Shopping Cost 16-20 Nov', 'reimburse', 'operasional', dsb). Gunakan ini jika user menyebut catatan atau periode mingguan!"
                            },
                            "item_contains": {
                                "type": "string",
                                "description": "Kata kunci pencarian nama transaksi atau merchant/toko (contoh: 'Shopee', 'Diamond Supermarket', 'Starbucks')."
                            },
                            "min_amount": {
                                "type": "integer",
                                "description": "Nominal minimum transaksi (IDR)."
                            },
                            "max_amount": {
                                "type": "integer",
                                "description": "Nominal maksimum transaksi (IDR)."
                            },
                            "date_from": {
                                "type": "string",
                                "description": "Filter tanggal mulai (format YYYY-MM-DD)."
                            },
                            "date_to": {
                                "type": "string",
                                "description": "Filter tanggal akhir (format YYYY-MM-DD)."
                            }
                        },
                        "required": []
                    }
                }
            }),
            json!({
                "type": "function",
                "function": {
                    "name": "update_transactions",
                    "description": "Perbarui status pembayaran, kategori, penanggung jawab, atau catatan pada transaksi tertentu. Harus menyertakan ref transaksi yang didapat dari list_transactions.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "refs": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Daftar referensi transaksi (ref) yang valid dari hasil list_transactions."
                            },
                            "status": {
                                "type": "string",
                                "enum": ["PAID", "PENDING", "UNPAID"],
                                "description": "Status baru."
                            },
                            "paid_by": {
                                "type": "string",
                                "enum": ["Wikra", "Dapur Bu Wikra", "Mbak Intan", "Vincent", "Kepin", "Teteh"],
                                "description": "Penanggung jawab baru."
                            },
                            "category": {
                                "type": "string",
                                "description": "Kategori baru."
                            },
                            "note": {
                                "type": "string",
                                "description": "Catatan tambahan."
                            }
                        },
                        "required": ["refs"]
                    }
                }
            }),
        ]
    }

    async fn execute_tool(&self, name: &str, arguments_raw: &str) -> Value {
        let args: Value = serde_json::from_str(arguments_raw).unwrap_or(json!({}));
        debug!("Agent calling tool {} with args {}", name, arguments_raw);

        match name {
            "list_months" => match self.mcp.list_months().await {
                Ok(months) => json!({ "months": months }),
                Err(e) => json!({ "error": e.to_string() }),
            },
            "summarize_transactions" => {
                let month = args.get("month").and_then(|v| v.as_str());
                let group_by = args.get("group_by").and_then(|v| v.as_str());
                match self.mcp.summarize_transactions(month, group_by).await {
                    Ok(val) => val,
                    Err(e) => json!({ "error": e.to_string() }),
                }
            }
            "list_transactions" => {
                let month = args.get("month").and_then(|v| v.as_str());
                let card = args.get("card").and_then(|v| v.as_str());
                let status = args.get("status").and_then(|v| v.as_str());
                let paid_by = args.get("paid_by").and_then(|v| v.as_str());
                let category = args.get("category").and_then(|v| v.as_str());
                let note = args.get("note_contains").and_then(|v| v.as_str());
                let item = args.get("item_contains").and_then(|v| v.as_str());
                let min_amount = args.get("min_amount").and_then(|v| v.as_u64());
                let max_amount = args.get("max_amount").and_then(|v| v.as_u64());
                let date_from = args.get("date_from").and_then(|v| v.as_str());
                let date_to = args.get("date_to").and_then(|v| v.as_str());

                match self.mcp.list_transactions(month, card, status, paid_by, category, note, item, min_amount, max_amount, date_from, date_to).await {
                    Ok(val) => val,
                    Err(e) => json!({ "error": e.to_string() }),
                }
            }
            "update_transactions" => {
                let refs = args.get("refs")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|r| r.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();
                let status = args.get("status").and_then(|v| v.as_str());
                let paid_by = args.get("paid_by").and_then(|v| v.as_str());
                let category = args.get("category").and_then(|v| v.as_str());
                let note = args.get("note").and_then(|v| v.as_str());

                match self.mcp.update_transactions(refs, status, paid_by, category, note).await {
                    Ok(val) => val,
                    Err(e) => json!({ "error": e.to_string() }),
                }
            }
            other => json!({ "error": format!("Tool '{other}' tidak dikenali") }),
        }
    }

    pub async fn chat(&self, channel_id: u64, user_prompt: &str) -> Result<String, anyhow::Error> {
        let system_prompt = r#"Kamu adalah Bon 🧾, asisten cerdas pencatat dan pengelola pengeluaran (expense tracker log) keluarga Wikra di Discord server Wikra Universe.
Tugas utamamu:
1. Mencatat pengeluaran harian dan memantau mutasi rekening/kartu kredit keluarga (BCA KrisFlyer, Mandiri Marriott Bonvoy, BNI Mastercard World, Jenius).
2. Memberikan ringkasan (rekap) pengeluaran bulanan, per kartu, per kategori, maupun per individu (Wikra, Dapur Bu Wikra, Mbak Intan, Vincent, Kepin, Teteh).
3. Membantu mencari transaksi/struk belanja (misal: "cek transaksi starbucks", "pengeluaran bca di atas 500rb", "mana transaksi yang pending?").
4. Mengupdate status pembayaran (PAID, PENDING, UNPAID) atau kategori pengeluaran jika diminta.

Pedoman Gaya Bahasa & Ketelitian Data:
- Selalu gunakan bahasa Indonesia yang ramah, ringkas, santai tapi terstruktur dan akurat. Panggil dirimu "Bon" atau "Si Bon 🧾".
- Selalu format nominal uang dalam format Rupiah yang mudah dibaca (misal: Rp 1.500.000).
- Sajikan rekap atau data transaksi dalam format list atau tabel Markdown yang rapi dengan emoji yang relevan.
- PENTING TENTANG FILTER PENCARIAN:
  * Jika user menyebutkan catatan/keperluan/periode mingguan (contoh: 'Shopping Cost 16-20 Nov', 'reimburse', 'operasional'), WAJIB gunakan parameter `note_contains`.
  * Jika user menyebutkan nama toko / merchant (contoh: 'Shopee', 'Diamond Supermarket', 'Starbucks'), gunakan `item_contains`.
- PENTING TENTANG ANGKA & PERHITUNGAN MATEMATIKA (ANTI-HALUSINASI):
  * Tool `list_transactions` dan `summarize_transactions` SUDAH menghitung angka total dan subtotal secara eksak dan akurat di level server (lihat field `total`, `count`, dan `by_card` di hasil JSON tool).
  * WAJIB gunakan angka dari field `total`, `count`, dan `by_card` yang disediakan oleh server response!
  * JANGAN PERNAH menghitung manual, menjumlahkan sendiri dengan mental math, atau menebak total nominal jika server sudah menyediakan angka pasti.
- Jika ada banyak transaksi (lebih dari 15), tampilkan ringkasan per kartu dari `by_card` dan daftarkan transaksi terpenting secara ringkas."#;

        let mut messages = vec![ChatMessage {
            role: "system".into(),
            content: Some(system_prompt.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }];

        // Load channel memory
        let history = self.memory.get_history(channel_id).await;
        messages.extend(history);

        // Add current user turn
        let user_msg = ChatMessage {
            role: "user".into(),
            content: Some(user_prompt.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        };
        messages.push(user_msg.clone());

        let url = format!("{}/chat/completions", self.config.llm_base_url.trim_end_matches('/'));
        let tools = self.tools_spec();

        // Multi-turn tool calling loop (up to 5 steps)
        let mut final_content = String::new();
        let mut loop_count = 0;

        while loop_count < 5 {
            loop_count += 1;

            let body = json!({
                "model": self.config.llm_model,
                "messages": messages,
                "tools": tools,
                "tool_choice": "auto",
                "temperature": 0.3,
            });

            debug!("Sending request to OpenRouter ({})", self.config.llm_model);

            let res = self.http.post(&url).json(&body).send().await?;
            if !res.status().is_success() {
                let status = res.status();
                let err_text = res.text().await.unwrap_or_default();
                error!("OpenRouter error {status}: {err_text}");
                return Err(anyhow::anyhow!("OpenRouter LLM error ({status}): {err_text}"));
            }

            let res_json: Value = res.json().await?;
            let choice = res_json["choices"]
                .as_array()
                .and_then(|arr| arr.first())
                .ok_or_else(|| anyhow::anyhow!("Empty choices in LLM response"))?;

            let message_obj = &choice["message"];
            let content_opt = message_obj["content"].as_str().map(|s| s.to_string());
            let tool_calls_opt = message_obj["tool_calls"].as_array();

            if let Some(tool_calls) = tool_calls_opt {
                if !tool_calls.is_empty() {
                    // Record assistant message with tool calls
                    let assistant_msg = ChatMessage {
                        role: "assistant".into(),
                        content: content_opt.clone(),
                        tool_calls: Some(tool_calls.clone()),
                        tool_call_id: None,
                        name: None,
                    };
                    messages.push(assistant_msg);

                    for tc in tool_calls {
                        let call_id = tc["id"].as_str().unwrap_or("call_default").to_string();
                        let func_name = tc["function"]["name"].as_str().unwrap_or("");
                        let func_args = tc["function"]["arguments"].as_str().unwrap_or("{}");

                        let result_val = self.execute_tool(func_name, func_args).await;

                        let tool_msg = ChatMessage {
                            role: "tool".into(),
                            content: Some(result_val.to_string()),
                            tool_calls: None,
                            tool_call_id: Some(call_id),
                            name: Some(func_name.to_string()),
                        };
                        messages.push(tool_msg);
                    }
                    continue; // Loop again so LLM can synthesize tool results
                }
            }

            // No tool calls: final answer reached
            if let Some(content) = content_opt {
                final_content = content;
            }
            break;
        }

        if final_content.is_empty() {
            final_content = "Maaf, saya tidak dapat memproses permintaan Anda saat ini.".to_string();
        }

        // Save turns to channel memory
        self.memory.append_message(channel_id, user_msg).await;
        self.memory.append_message(channel_id, ChatMessage {
            role: "assistant".into(),
            content: Some(final_content.clone()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }).await;

        Ok(final_content)
    }
}
