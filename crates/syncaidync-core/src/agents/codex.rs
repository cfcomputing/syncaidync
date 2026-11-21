//! Codex adapter. Observes by activity; notify hook speeds up TurnEnded.

use serde_json::Value;

use crate::model::Usage;
use crate::signal::Signal;
use crate::startup::Marker;

use super::{Adapter, AgentId, LaunchPlan, LaunchRequest, Observes, OneShotPlan};

const MARKERS: &[Marker] = &[
    Marker {
        needle: "sign in with chatgpt",
        message: "Needs you to sign in",
    },
    Marker {
        needle: "do you trust",
        message: "Asking whether to trust this folder",
    },
];

#[derive(Debug, Default, Clone, Copy)]
pub struct CodexAdapter;

impl Adapter for CodexAdapter {
    fn id(&self) -> AgentId {
        AgentId::Codex
    }

    fn label(&self) -> &'static str {
        "Codex"
    }

    fn bin_names(&self) -> &'static [&'static str] {
        &["codex"]
    }

    fn observes(&self) -> Observes {
        // Notify supplies TurnEnded; activity covers the rest.
        Observes::Activity
    }

    fn models(&self) -> &'static [&'static str] {
        &[]
    }

    fn launch(&self, req: &LaunchRequest<'_>) -> LaunchPlan {
        let mut args = vec!["codex".into()];
        if req.resume {
            args.push("resume".into());
            if let Some(id) = req.agent_session {
                args.push(id.into());
            } else {
                args.push("--last".into());
            }
        }
        if let Some(model) = req.model.filter(|m| !m.is_empty()) {
            args.push("-m".into());
            args.push(model.into());
        }
        if req.autonomy {
            args.push("--dangerously-bypass-approvals-and-sandbox".into());
        }
        // Turn-complete notify → syncaidync signal --agent codex
        let notify = format!(
            r#"notify=["{}","signal","--agent","codex"]"#,
            escape_toml_str(req.hook_bin)
        );
        args.push("-c".into());
        args.push(notify);
        // Per-request tokens → syncaidync usage --agent codex (observation-only hook).
        let usage_cmd = format!(
            r#"hooks.PostModelRequest=[{{hooks=[{{type="command",command="\"{}\" usage --agent codex",timeout=5,async=true}}]}}]"#,
            escape_toml_str(req.hook_bin)
        );
        args.push("-c".into());
        args.push(usage_cmd);
        args.push("--dangerously-bypass-hook-trust".into());

        if !req.resume {
            if let Some(prompt) = req.prompt.filter(|p| !p.is_empty()) {
                args.push(prompt.into());
            }
        }

        LaunchPlan {
            args,
            env: Vec::new(),
            files: Vec::new(),
            agent_session: None,
        }
    }

    fn signals(&self, payload: &Value) -> Vec<Signal> {
        codex_signals(payload)
    }

    fn usage(&self, payload: &Value) -> Option<Usage> {
        codex_usage(payload)
    }

    fn startup_markers(&self) -> &'static [Marker] {
        MARKERS
    }

    fn oneshot(&self, model: Option<&str>, prompt: &str) -> OneShotPlan {
        let mut args = vec![
            "codex".into(),
            "exec".into(),
            "--ephemeral".into(),
            "--sandbox".into(),
            "read-only".into(),
            "--skip-git-repo-check".into(),
            "--json".into(),
        ];
        if let Some(m) = model.filter(|m| !m.is_empty()) {
            args.push("-m".into());
            args.push(m.into());
        }
        args.push("-".into());
        OneShotPlan {
            args,
            stdin: Some(prompt.to_string()),
        }
    }

    fn oneshot_reply(&self, stdout: &str) -> Option<String> {
        // Last item.completed with agent_message text.
        let mut last = None;
        for line in stdout.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if v.get("type").and_then(|t| t.as_str()) == Some("item.completed") {
                let item = v.get("item")?;
                if item.get("type").and_then(|t| t.as_str()) == Some("agent_message") {
                    if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                        last = Some(t.to_string());
                    }
                }
            }
        }
        last
    }
}

fn escape_toml_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn json_u64(v: &Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_i64().and_then(|n| u64::try_from(n).ok()))
        .or_else(|| v.as_f64().and_then(|n| (n >= 0.0).then_some(n as u64)))
}

/// Tokens for one model attempt. Prefer `total_tokens`; otherwise sum the parts.
pub fn codex_usage(payload: &Value) -> Option<Usage> {
    let event = payload
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if event != "PostModelRequest" {
        // Also accept a bare token_usage object for tests / future notify shapes.
        if payload.get("token_usage").is_none() && payload.get("tokens").is_none() {
            return None;
        }
    }
    if let Some(n) = payload.get("tokens").and_then(json_u64) {
        return Some(Usage {
            cost_usd: None,
            context_pct: None,
            tokens: Some(n),
        });
    }
    let usage = payload.get("token_usage")?;
    if usage.is_null() {
        return None;
    }
    let tokens = usage.get("total_tokens").and_then(json_u64).or_else(|| {
        let input = usage.get("input_tokens").and_then(json_u64).unwrap_or(0);
        let output = usage.get("output_tokens").and_then(json_u64).unwrap_or(0);
        let reasoning = usage
            .get("reasoning_output_tokens")
            .and_then(json_u64)
            .unwrap_or(0);
        let n = input.saturating_add(output).saturating_add(reasoning);
        (n > 0).then_some(n)
    })?;
    Some(Usage {
        cost_usd: None,
        context_pct: None,
        tokens: Some(tokens),
    })
}

pub fn codex_signals(payload: &Value) -> Vec<Signal> {
    let ty = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if ty == "agent-turn-complete" {
        let mut out = vec![Signal::TurnEnded];
        if let Some(id) = payload
            .get("thread-id")
            .or_else(|| payload.get("thread_id"))
            .or_else(|| payload.get("conversation_id"))
            .and_then(|v| v.as_str())
        {
            out.insert(
                0,
                Signal::SessionStarted {
                    agent_session: Some(id.to_string()),
                    transcript: None,
                },
            );
        }
        return out;
    }
    // Hook-shaped payloads (if config hooks land later).
    if let Some(event) = payload.get("hook_event_name").and_then(|v| v.as_str()) {
        return match event {
            "SessionStart" | "session_start" => vec![Signal::SessionStarted {
                agent_session: payload
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                transcript: None,
            }],
            "UserPromptSubmit" | "prompt" => vec![Signal::PromptSubmitted],
            "Stop" | "turn_complete" => vec![Signal::TurnEnded],
            _ => vec![Signal::Other],
        };
    }
    vec![Signal::Other]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oneshot_args_and_jsonl_reply() {
        let a = CodexAdapter;
        let plan = a.oneshot(Some("gpt"), "hi");
        assert!(plan.args.contains(&"exec".into()));
        assert!(plan.args.contains(&"--json".into()));
        assert_eq!(plan.args.last().map(String::as_str), Some("-"));
        let stdout = r#"{"type":"item.completed","item":{"type":"agent_message","text":"first"}}
{"type":"item.completed","item":{"type":"agent_message","text":"last"}}
"#;
        assert_eq!(a.oneshot_reply(stdout).as_deref(), Some("last"));
    }

    #[test]
    fn launch_variants() {
        let a = CodexAdapter;
        let start = a.launch(&LaunchRequest {
            session_id: "s",
            agent_session: None,
            resume: false,
            model: Some("o3"),
            autonomy: true,
            prompt: Some("hi"),
            hook_bin: "/bin/syncaidync",
            run_dir: "/tmp",
        });
        assert!(start.args.contains(&"-m".into()));
        assert!(start.args.contains(&"o3".into()));
        assert!(start
            .args
            .iter()
            .any(|a| a.contains("--dangerously-bypass-approvals")));
        assert!(start.args.contains(&"hi".into()));
        assert!(start.args.iter().any(|a| a.starts_with("notify=")));
        assert!(start
            .args
            .iter()
            .any(|a| a.contains("hooks.PostModelRequest=") && a.contains("usage --agent codex")));

        let resume = a.launch(&LaunchRequest {
            session_id: "s",
            agent_session: Some("tid"),
            resume: true,
            model: None,
            autonomy: false,
            prompt: Some("nope"),
            hook_bin: "/bin/syncaidync",
            run_dir: "/tmp",
        });
        assert!(resume.args.contains(&"resume".into()));
        assert!(resume.args.contains(&"tid".into()));
        assert!(!resume.args.iter().any(|a| a == "nope"));
    }

    #[test]
    fn notify_turn_complete() {
        let a = CodexAdapter;
        let sigs = a.signals(&serde_json::json!({
            "type": "agent-turn-complete",
            "thread-id": "t1"
        }));
        assert!(sigs.iter().any(|s| matches!(s, Signal::TurnEnded)));
        assert!(sigs.iter().any(|s| matches!(
            s,
            Signal::SessionStarted {
                agent_session: Some(id),
                ..
            } if id == "t1"
        )));
    }

    #[test]
    fn usage_from_post_model_request() {
        let a = CodexAdapter;
        let u = a
            .usage(&serde_json::json!({
                "hook_event_name": "PostModelRequest",
                "status": "completed",
                "token_usage": {
                    "input_tokens": 40,
                    "cached_input_tokens": 0,
                    "cache_write_input_tokens": 0,
                    "output_tokens": 2,
                    "reasoning_output_tokens": 0,
                    "total_tokens": 42
                }
            }))
            .unwrap();
        assert_eq!(u.tokens, Some(42));
        assert!(a
            .usage(&serde_json::json!({
                "hook_event_name": "PostModelRequest",
                "token_usage": null
            }))
            .is_none());
    }
}
