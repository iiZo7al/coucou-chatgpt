// ChatGPT-plan chat through the locally installed OpenAI Codex CLI.
// This uses the user's Codex/ChatGPT login instead of an API key.

use std::os::windows::process::CommandExt;
use std::process::Command;
use serde_json::Value;
use crate::openai::{ChatContext, ChatReply};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn codex_exe() -> Option<std::path::PathBuf> {
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    let dirs = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&dirs) {
        for ext in exts.split(';').filter(|e| !e.is_empty()) {
            let p = dir.join(format!("codex{}", ext.to_lowercase()));
            if p.is_file() { return Some(p); }
        }
    }
    None
}

pub fn installed() -> bool { codex_exe().is_some() }

pub fn login_status() -> Result<String, String> {
    let exe = codex_exe().ok_or("Codex CLI is not installed. Install Codex first.")?;
    let out = Command::new(exe).args(["login", "status"]).creation_flags(CREATE_NO_WINDOW)
        .output().map_err(|e| e.to_string())?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.success() { Ok(text.trim().to_string()) } else { Err(text.trim().to_string()) }
}

pub fn start_login() -> Result<(), String> {
    let exe = codex_exe().ok_or("Codex CLI is not installed. Install Codex first.")?;
    Command::new(exe).arg("login").creation_flags(CREATE_NO_WINDOW)
        .spawn().map(|_| ()).map_err(|e| e.to_string())
}

pub async fn send(model: &str, query: String, context: Option<ChatContext>) -> Result<ChatReply, String> {
    let exe = codex_exe().ok_or("Codex CLI is not installed. Install it, then Continue with ChatGPT.")?;
    let mut prompt = String::from("You are Mochi, a concise personal assistant. Respond in the user's language. ");
    match context {
        Some(ChatContext::File { name, path }) => {
            prompt.push_str(&format!("The user attached file {name} at this local path: {path}. Read it only if needed. "));
        }
        Some(ChatContext::Window { app_name, title, url }) => {
            prompt.push_str(&format!("Window context: app={app_name}, title={title}"));
            if let Some(url) = url { prompt.push_str(&format!(", url={url}")); }
            prompt.push_str(". ");
        }
        None => {}
    }
    prompt.push_str(&query);

    let model = model.to_string();
    tokio::task::spawn_blocking(move || {
        let out = Command::new(exe)
            .args(["exec", "--json", "--sandbox", "read-only", "--skip-git-repo-check", "--model", &model, &prompt])
            .creation_flags(CREATE_NO_WINDOW)
            .output().map_err(|e| format!("Could not start Codex: {e}"))?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !out.status.success() {
            return Err(format!("Codex: {}", if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() }));
        }
        let mut answer = String::new();
        for line in stdout.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            if v.get("type").and_then(Value::as_str) == Some("item.completed") {
                if let Some(item) = v.get("item") {
                    if item.get("type").and_then(Value::as_str) == Some("agent_message") {
                        if let Some(text) = item.get("text").and_then(Value::as_str) { answer = text.to_string(); }
                    }
                }
            }
        }
        if answer.is_empty() { Err("Codex completed without an assistant message.".into()) }
        else { Ok(ChatReply { text: answer }) }
    }).await.map_err(|e| e.to_string())?
}
