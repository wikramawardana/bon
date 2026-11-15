use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tracing::{debug, error};

#[derive(Clone, Debug)]
pub struct McpClient {
    client: reqwest::Client,
    base_url: String,
    #[allow(dead_code)]
    token: String,
}

#[derive(Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Deserialize, Debug)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    #[allow(dead_code)]
    id: Option<Value>,
    result: Option<McpResult>,
    error: Option<McpError>,
}

#[derive(Deserialize, Debug)]
struct McpResult {
    content: Option<Vec<McpContent>>,
    #[serde(rename = "structuredContent")]
    structured_content: Option<Value>,
    #[serde(rename = "isError")]
    is_error: Option<bool>,
}

#[derive(Deserialize, Debug)]
struct McpContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Deserialize, Debug)]
struct McpError {
    #[allow(dead_code)]
    code: i64,
    message: String,
}

impl McpClient {
    pub fn new(base_url: String, token: String) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {token}")) {
            headers.insert(AUTHORIZATION, val);
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(45))
            .build()
            .unwrap_or_default();

        Self {
            client,
            base_url,
            token,
        }
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, anyhow::Error> {
        let payload = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "tools/call",
            params: serde_json::json!({
                "name": name,
                "arguments": arguments,
            }),
        };

        debug!("Calling MCP tool '{}' with args: {:?}", name, arguments);

        let res = self
            .client
            .post(&self.base_url)
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            error!("MCP HTTP Error {status}: {body}");
            return Err(anyhow::anyhow!("MCP HTTP error {status}: {body}"));
        }

        let rpc_res: JsonRpcResponse = res.json().await?;

        if let Some(err) = rpc_res.error {
            return Err(anyhow::anyhow!("MCP RPC Error: {}", err.message));
        }

        let result = rpc_res
            .result
            .ok_or_else(|| anyhow::anyhow!("Empty result from MCP"))?;

        if result.is_error.unwrap_or(false) {
            let err_msg = result
                .content
                .and_then(|c| c.into_iter().next())
                .and_then(|c| c.text)
                .unwrap_or_else(|| "Unknown MCP error".into());
            return Err(anyhow::anyhow!("MCP tool failed: {err_msg}"));
        }

        if let Some(structured) = result.structured_content {
            return Ok(structured);
        }

        if let Some(content) = result.content {
            for c in content {
                if c.content_type == "text" {
                    if let Some(text) = c.text {
                        if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                            return Ok(parsed);
                        }
                        return Ok(Value::String(text));
                    }
                }
            }
        }

        Ok(Value::Null)
    }

    pub async fn list_months(&self) -> Result<Vec<String>, anyhow::Error> {
        let val = self.call_tool("list_months", serde_json::json!({})).await?;
        if let Some(tabs) = val.get("tabs").and_then(|t| t.as_array()) {
            let res = tabs
                .iter()
                .filter_map(|t| t.as_str().map(|s| s.to_string()))
                .collect();
            return Ok(res);
        }
        Ok(Vec::new())
    }

    pub async fn summarize_transactions(
        &self,
        month: Option<&str>,
        group_by: Option<&str>,
    ) -> Result<Value, anyhow::Error> {
        let mut args = serde_json::Map::new();
        if let Some(m) = month {
            args.insert("month".to_string(), Value::String(m.to_string()));
        }
        if let Some(g) = group_by {
            args.insert("group_by".to_string(), Value::String(g.to_string()));
        }
        self.call_tool("summarize_transactions", Value::Object(args)).await
    }

    pub async fn list_transactions(
        &self,
        month: Option<&str>,
        card: Option<&str>,
        status: Option<&str>,
        paid_by: Option<&str>,
        min_amount: Option<u64>,
        max_amount: Option<u64>,
        item_contains: Option<&str>,
    ) -> Result<Value, anyhow::Error> {
        let mut args = serde_json::Map::new();
        if let Some(m) = month {
            args.insert("month".to_string(), Value::String(m.to_string()));
        }
        if let Some(c) = card {
            args.insert("card".to_string(), Value::String(c.to_string()));
        }
        if let Some(s) = status {
            args.insert("status".to_string(), Value::String(s.to_string()));
        }
        if let Some(p) = paid_by {
            args.insert("paid_by".to_string(), Value::String(p.to_string()));
        }
        if let Some(min) = min_amount {
            args.insert("min_amount".to_string(), Value::Number(min.into()));
        }
        if let Some(max) = max_amount {
            args.insert("max_amount".to_string(), Value::Number(max.into()));
        }
        if let Some(item) = item_contains {
            args.insert("item_contains".to_string(), Value::String(item.to_string()));
        }
        self.call_tool("list_transactions", Value::Object(args)).await
    }

    pub async fn update_transactions(
        &self,
        refs: Vec<String>,
        status: Option<&str>,
        paid_by: Option<&str>,
        category: Option<&str>,
        note: Option<&str>,
    ) -> Result<Value, anyhow::Error> {
        let mut args = serde_json::Map::new();
        args.insert("refs".to_string(), serde_json::json!(refs));
        if let Some(s) = status {
            args.insert("status".to_string(), Value::String(s.to_string()));
        }
        if let Some(p) = paid_by {
            args.insert("paid_by".to_string(), Value::String(p.to_string()));
        }
        if let Some(c) = category {
            args.insert("category".to_string(), Value::String(c.to_string()));
        }
        if let Some(n) = note {
            args.insert("note".to_string(), Value::String(n.to_string()));
        }
        self.call_tool("update_transactions", Value::Object(args)).await
    }

    pub async fn check_health(&self) -> bool {
        match self.list_months().await {
            Ok(_) => true,
            Err(e) => {
                error!("Health check failed for MCP: {e}");
                false
            }
        }
    }
}
