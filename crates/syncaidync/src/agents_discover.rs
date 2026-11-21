//! Find agent CLIs on PATH and known install locations, and list their models.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use syncaidync_core::{adapter_for, Adapter, AgentId, Observes, ShellAdapter};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

struct Cache {
    at: Instant,
    agents: Vec<Value>,
}

struct ModelsCache {
    at: Instant,
    by_agent: HashMap<&'static str, Vec<String>>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
static MODELS_CACHE: Mutex<Option<ModelsCache>> = Mutex::new(None);

const BIN_TTL: Duration = Duration::from_secs(30);
const MODELS_TTL: Duration = Duration::from_secs(120);
const LIST_TIMEOUT: Duration = Duration::from_secs(4);
const MAX_MODELS: usize = 500;

fn env_override(id: AgentId) -> Option<PathBuf> {
    let key = match id {
        AgentId::Claude => "SYNCAIDYNC_BIN_CLAUDE",
        AgentId::Codex => "SYNCAIDYNC_BIN_CODEX",
        AgentId::Pi => "SYNCAIDYNC_BIN_PI",
        AgentId::Cursor => "SYNCAIDYNC_BIN_CURSOR",
        AgentId::Shell => return None,
    };
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        for p in std::env::split_paths(&path) {
            dirs.push(p);
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(home) = &home {
        for rel in [".local/bin", ".cargo/bin", ".bun/bin", ".npm-global/bin"] {
            dirs.push(home.join(rel));
        }
    }
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs
}

fn find_on_path(names: &[&str]) -> Option<PathBuf> {
    let dirs = search_dirs();
    for name in names {
        for dir in &dirs {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn find_bin(id: AgentId) -> Option<PathBuf> {
    if let Some(p) = env_override(id) {
        return Some(p);
    }
    let adapter = adapter_for(id)?;
    find_on_path(adapter.bin_names())
}

pub fn resolve_bin_for_launch(id: AgentId, plan_bin: &str) -> Result<String, String> {
    if let Some(p) = find_bin(id) {
        return Ok(p.to_string_lossy().into_owned());
    }
    // Fall back to whatever the plan put first (may work if PATH is set for the child).
    if Path::new(plan_bin).is_file() {
        return Ok(plan_bin.to_string());
    }
    // Try bare name on PATH via which-style
    if find_on_path(&[plan_bin]).is_some() {
        return Ok(plan_bin.to_string());
    }
    Err(format!(
        "Could not find the {} CLI. Install it or set SYNCAIDYNC_BIN_{}.",
        id.as_str(),
        id.as_str().to_uppercase()
    ))
}

fn agent_json(
    id: AgentId,
    adapter: &dyn Adapter,
    bin: Option<PathBuf>,
    models: &[String],
) -> Value {
    let installed = bin.is_some();
    json!({
        "id": id.as_str(),
        "label": adapter.label(),
        "installed": installed,
        "bin": bin.map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| {
            adapter.bin_names().first().copied().unwrap_or("").to_string()
        }),
        "observes": match adapter.observes() {
            Observes::Signals => "signals",
            Observes::Activity => "activity",
        },
        "models": models,
        "history": matches!(id, AgentId::Claude),
    })
}

fn static_models(adapter: &dyn Adapter) -> Vec<String> {
    adapter.models().iter().map(|s| (*s).to_string()).collect()
}

/// Parse `pi --list-models` table output into `provider/model` ids.
pub fn parse_pi_models(stdout: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("Warning:") {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(provider) = parts.next() else {
            continue;
        };
        let Some(model) = parts.next() else {
            continue;
        };
        if provider.eq_ignore_ascii_case("provider") && model.eq_ignore_ascii_case("model") {
            continue;
        }
        if !provider
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            continue;
        }
        let id = format!("{provider}/{model}");
        if !out.iter().any(|m| m == &id) {
            out.push(id);
        }
        if out.len() >= MAX_MODELS {
            break;
        }
    }
    out
}

/// Parse `agent --list-models` / `cursor-agent --list-models` output (`id - Label`).
pub fn parse_cursor_models(stdout: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.eq_ignore_ascii_case("Available models") {
            continue;
        }
        if line.starts_with("Failed to load") || line.starts_with("Error") {
            continue;
        }
        let Some((id, _)) = line.split_once(" - ") else {
            continue;
        };
        let id = id.trim();
        if id.is_empty() || id.contains(' ') {
            continue;
        }
        if !out.iter().any(|m| m == id) {
            out.push(id.to_string());
        }
        if out.len() >= MAX_MODELS {
            break;
        }
    }
    out
}

async fn capture_stdout(bin: &Path, args: &[&str]) -> Option<String> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let read = async {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).await.ok()?;
        String::from_utf8(buf).ok()
    };
    match timeout(LIST_TIMEOUT, read).await {
        Ok(Some(text)) => {
            let _ = timeout(Duration::from_millis(500), child.wait()).await;
            Some(text)
        }
        _ => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            None
        }
    }
}

async fn discover_models(id: AgentId, bin: &Path) -> Option<Vec<String>> {
    match id {
        AgentId::Pi => {
            let text = capture_stdout(bin, &["--list-models"]).await?;
            let models = parse_pi_models(&text);
            if models.is_empty() {
                None
            } else {
                Some(models)
            }
        }
        AgentId::Cursor => {
            let text = capture_stdout(bin, &["--list-models"]).await?;
            let models = parse_cursor_models(&text);
            if models.is_empty() {
                None
            } else {
                Some(models)
            }
        }
        AgentId::Claude | AgentId::Codex | AgentId::Shell => None,
    }
}

fn cached_models() -> Option<HashMap<&'static str, Vec<String>>> {
    let guard = MODELS_CACHE.lock().ok()?;
    let cache = guard.as_ref()?;
    if cache.at.elapsed() < MODELS_TTL {
        Some(cache.by_agent.clone())
    } else {
        None
    }
}

fn store_models(by_agent: HashMap<&'static str, Vec<String>>) {
    if let Ok(mut guard) = MODELS_CACHE.lock() {
        *guard = Some(ModelsCache {
            at: Instant::now(),
            by_agent,
        });
    }
}

async fn models_for_installed(
    installed: &[(AgentId, PathBuf)],
) -> HashMap<&'static str, Vec<String>> {
    if let Some(cached) = cached_models() {
        return cached;
    }

    let pi_bin = installed
        .iter()
        .find(|(id, _)| *id == AgentId::Pi)
        .map(|(_, b)| b.clone());
    let cursor_bin = installed
        .iter()
        .find(|(id, _)| *id == AgentId::Cursor)
        .map(|(_, b)| b.clone());

    let pi_fut = async {
        match pi_bin {
            Some(bin) => discover_models(AgentId::Pi, &bin).await,
            None => None,
        }
    };
    let cursor_fut = async {
        match cursor_bin {
            Some(bin) => discover_models(AgentId::Cursor, &bin).await,
            None => None,
        }
    };
    let (pi_models, cursor_models) = tokio::join!(pi_fut, cursor_fut);

    let mut by_agent = HashMap::new();
    if let Some(models) = pi_models {
        by_agent.insert(AgentId::Pi.as_str(), models);
    }
    if let Some(models) = cursor_models {
        by_agent.insert(AgentId::Cursor.as_str(), models);
    }
    store_models(by_agent.clone());
    by_agent
}

fn build_agents(dev_shell: bool, discovered: &HashMap<&'static str, Vec<String>>) -> Vec<Value> {
    let mut agents = Vec::new();
    for id in [
        AgentId::Claude,
        AgentId::Codex,
        AgentId::Pi,
        AgentId::Cursor,
    ] {
        if let Some(adapter) = adapter_for(id) {
            let bin = find_bin(id);
            let models = discovered
                .get(id.as_str())
                .cloned()
                .unwrap_or_else(|| static_models(adapter));
            agents.push(agent_json(id, adapter, bin, &models));
        }
    }
    if dev_shell {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let adapter = ShellAdapter::new(shell.clone());
        agents.push(json!({
            "id": "shell",
            "label": adapter.label(),
            "installed": true,
            "bin": shell,
            "observes": "activity",
            "models": [],
            "history": false,
        }));
    }
    agents
}

pub async fn list_agents(dev_shell: bool) -> Vec<Value> {
    if let Ok(guard) = CACHE.lock() {
        if let Some(c) = guard.as_ref() {
            if c.at.elapsed() < BIN_TTL {
                return c.agents.clone();
            }
        }
    }

    let installed: Vec<(AgentId, PathBuf)> = [
        AgentId::Claude,
        AgentId::Codex,
        AgentId::Pi,
        AgentId::Cursor,
    ]
    .into_iter()
    .filter_map(|id| find_bin(id).map(|bin| (id, bin)))
    .collect();

    let discovered = models_for_installed(&installed).await;
    let agents = build_agents(dev_shell, &discovered);

    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some(Cache {
            at: Instant::now(),
            agents: agents.clone(),
        });
    }
    agents
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pi_table() {
        let raw = "\
Warning: something
provider        model                         context  max-out
amazon-bedrock  amazon.nova-lite-v1:0         300K     8.2K
openai          gpt-4o                        128K     16K
";
        assert_eq!(
            parse_pi_models(raw),
            vec![
                "amazon-bedrock/amazon.nova-lite-v1:0".to_string(),
                "openai/gpt-4o".to_string(),
            ]
        );
    }

    #[test]
    fn parses_cursor_list() {
        let raw = "\
Available models

auto - Auto (current, default)
gpt-5.2 - GPT-5.2
Failed to load models: nope
composer-2.5 - Composer 2.5
";
        assert_eq!(
            parse_cursor_models(raw),
            vec![
                "auto".to_string(),
                "gpt-5.2".to_string(),
                "composer-2.5".to_string(),
            ]
        );
    }
}
