// OpenBot companion integration for Coucou on Windows.
//
// OpenBot keeps provider credentials in the provider-owned CLI state (for Codex,
// ~/.codex). Coucou never copies those credentials. We detect and launch the
// installed OpenBot desktop app, then use the same local Codex login/runtime
// state for Mochi chat. This avoids OpenAI API billing and keeps OpenBot as the
// place where providers and models are managed.

use serde::Serialize;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenBotStatus {
    pub installed: bool,
    pub running: bool,
    pub path: Option<String>,
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        out.push(local.join("Programs").join("OpenBot").join("OpenBot.exe"));
        out.push(local.join("OpenBot").join("OpenBot.exe"));
    }
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        out.push(PathBuf::from(program_files).join("OpenBot").join("OpenBot.exe"));
    }
    if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
        out.push(PathBuf::from(program_files_x86).join("OpenBot").join("OpenBot.exe"));
    }
    out
}

fn executable() -> Option<PathBuf> {
    candidates().into_iter().find(|p| p.is_file())
}

fn tasklist_contains(exe_name: &str) -> bool {
    Command::new("tasklist.exe")
        .args(["/FI", &format!("IMAGENAME eq {exe_name}"), "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_ascii_lowercase().contains(&exe_name.to_ascii_lowercase()))
        .unwrap_or(false)
}

pub fn status() -> OpenBotStatus {
    let exe = executable();
    OpenBotStatus {
        installed: exe.is_some(),
        running: tasklist_contains("OpenBot.exe"),
        path: exe.as_ref().map(|p| p.to_string_lossy().to_string()),
    }
}

pub fn launch() -> Result<(), String> {
    if tasklist_contains("OpenBot.exe") {
        return Ok(());
    }
    let exe = executable().ok_or_else(|| {
        "OpenBot is not installed. Install OpenBot for Windows, then try again.".to_string()
    })?;
    Command::new(&exe)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not launch {}: {e}", display(&exe)))
}

fn display(path: &Path) -> String {
    path.to_string_lossy().to_string()
}
