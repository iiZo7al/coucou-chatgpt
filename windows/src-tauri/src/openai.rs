// OpenAI Responses API client for Coucou on Windows.
// Keeps the existing module name so the rest of the app does not need a risky refactor.

use std::sync::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::secrets;

const ENDPOINT: &str = "https://api.openai.com/v1/responses";
const MAX_OUTPUT_TOKENS: u32 = 4096;
const MAX_INLINE_TEXT: u64 = 200_000;

pub const DEFAULT_MODEL: &str = "gpt-5.6-sol";

const SYSTEM_PROMPT: &str = "You are Mochi, a personal AI assistant living at the top of the user's screen. \
You have web search access and can help with research, coding, finding places, recommendations, tasks, and questions. \
Respond in the user's language. Be thorough and complete. \
No markdown formatting (no **, no ##, no bullet dashes). Use plain text with line breaks.";

#[derive(Default)]
pub struct Chat {
    messages: Mutex<Vec<Value>>,
}

impl Chat {
    pub fn reset(&self) { self.messages.lock().unwrap().clear(); }
    fn is_empty(&self) -> bool { self.messages.lock().unwrap().is_empty() }
    fn push(&self, message: Value) { self.messages.lock().unwrap().push(message); }
    fn pop(&self) { self.messages.lock().unwrap().pop(); }
    fn snapshot(&self) -> Vec<Value> { self.messages.lock().unwrap().clone() }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatContext {
    File { name: String, path: String },
    Window { app_name: String, title: String, url: Option<String> },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatReply { pub text: String }

pub async fn send(
    chat: &Chat,
    model: &str,
    query: String,
    context: Option<ChatContext>,
) -> Result<ChatReply, String> {
    let key = secrets::get("openai-api-key")
        .ok_or_else(|| "OpenAI API key missing. Open settings.".to_string())?;

    let mut content: Vec<Value> = Vec::new();
    if chat.is_empty() {
        match &context {
            Some(ChatContext::File { name, path }) => {
                if let Some(block) = file_block(name, path) { content.push(block); }
                content.push(json!({ "type": "input_text", "text": format!("File: {name}") }));
            }
            Some(ChatContext::Window { app_name, title, url }) => {
                let mut text = format!("Context — App: {app_name}, Window: {title}");
                if let Some(url) = url { text.push_str(&format!(", URL: {url}")); }
                content.push(json!({ "type": "input_text", "text": text }));
            }
            None => {}
        }
    }
    content.push(json!({ "type": "input_text", "text": query }));
    chat.push(json!({ "role": "user", "content": content }));

    let body = json!({
        "model": model,
        "instructions": SYSTEM_PROMPT,
        "input": chat.snapshot(),
        "tools": [{ "type": "web_search" }],
        "max_output_tokens": MAX_OUTPUT_TOKENS
    });

    let response = match call(&key, &body).await {
        Ok(v) => v,
        Err(err) => { chat.pop(); return Err(err); }
    };

    let text = extract_output_text(&response)
        .ok_or_else(|| "No response text.".to_string())?;
    chat.push(json!({ "role": "assistant", "content": text }));
    Ok(ChatReply { text })
}

fn extract_output_text(response: &Value) -> Option<String> {
    let mut parts = Vec::new();
    for item in response.get("output")?.as_array()? {
        if item.get("type").and_then(Value::as_str) != Some("message") { continue; }
        if let Some(content) = item.get("content").and_then(Value::as_array) {
            for block in content {
                if block.get("type").and_then(Value::as_str) == Some("output_text") {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        parts.push(text);
                    }
                }
            }
        }
    }
    let text = parts.join("\n").trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

async fn call(key: &str, body: &Value) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .build().map_err(|e| e.to_string())?;

    let response = client.post(ENDPOINT)
        .bearer_auth(key)
        .header("content-type", "application/json")
        .json(body)
        .send().await
        .map_err(|e| format!("Network error: {e}"))?;

    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let detail = serde_json::from_str::<Value>(&text).ok()
            .and_then(|v| v.get("error")?.get("message")?.as_str().map(str::to_string))
            .unwrap_or_else(|| text.chars().take(300).collect());
        return Err(format!("OpenAI API {status}: {detail}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("Bad API response: {e}"))
}

fn file_block(name: &str, path: &str) -> Option<Value> {
    let ext = std::path::Path::new(path).extension()
        .and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    if ext == "pdf" {
        let bytes = std::fs::read(path).ok()?;
        return Some(json!({
            "type": "input_file",
            "filename": name,
            "file_data": format!("data:application/pdf;base64,{}", base64(&bytes))
        }));
    }

    let media = match ext.as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    };
    if let Some(media) = media {
        let bytes = std::fs::read(path).ok()?;
        return Some(json!({
            "type": "input_image",
            "image_url": format!("data:{media};base64,{}", base64(&bytes))
        }));
    }

    let len = std::fs::metadata(path).ok()?.len();
    if len > MAX_INLINE_TEXT { return None; }
    let text = std::fs::read_to_string(path).ok()?;
    Some(json!({ "type": "input_text", "text": format!("File contents:\n{text}") }))
}

pub(crate) fn base64_for(bytes: &[u8]) -> String { base64(bytes) }

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64;
    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
