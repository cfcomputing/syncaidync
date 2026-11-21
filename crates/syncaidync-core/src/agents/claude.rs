//! Claude Code adapter.

use serde_json::{json, Value};

use crate::model::Usage;
use crate::signal::Signal;
use crate::startup::Marker;

use super::detail::tool_detail;
use super::{Adapter, AgentId, LaunchPlan, LaunchRequest, Observes, OneShotPlan};

const MARKERS: &[Marker] = &[
    Marker {
        needle: "do you trust the files in this folder",
        message: "Asking whether to trust this folder",
    },
    Marker {
        needle: "bypass permissions mode",
        message: "Asking you to accept running without approval prompts",
    },
    Marker {
        needle: "select login method",
        message: "Needs you to sign in",
    },
];

#[derive(Debug, Default, Clone, Copy)]
pub struct ClaudeAdapter;

impl Adapter for ClaudeAdapter {
    fn id(&self) -> AgentId {
        AgentId::Claude
    }

    fn label(&self) -> &'static str {
        "Claude Code"
    }

    fn bin_names(&self) -> &'static [&'static str] {
        &["claude"]
    }

    fn observes(&self) -> Observes {
        Observes::Signals
    }

    fn models(&self) -> &'static [&'static str] {
        &["opus", "sonnet", "haiku"]
    }

    fn launch(&self, req: &LaunchRequest<'_>) -> LaunchPlan {
        let settings_path = format!("{}/claude-settings.json", req.run_dir.trim_end_matches('/'));
        let settings = claude_settings_json(req.hook_bin);
        let mut args = vec!["claude".into()];

        let mut agent_session = None;
        if req.resume {
            if let Some(id) = req.agent_session {
                args.push("--resume".into());
                args.push(id.into());
            } else {
                args.push("--continue".into());
            }
        } else {
            let id = req.session_id.to_string();
            args.push("--session-id".into());
            args.push(id.clone());
            agent_session = Some(id);
        }

        args.push("--settings".into());
        args.push(settings_path.clone());

        if let Some(model) = req.model.filter(|m| !m.is_empty()) {
            args.push("--model".into());
            args.push(model.into());
        }
        if req.autonomy {
            args.push("--dangerously-skip-permissions".into());
        }
        if !req.resume {
            if let Some(prompt) = req.prompt.filter(|p| !p.is_empty()) {
                args.push(prompt.into());
            }
        }

        LaunchPlan {
            args,
            env: Vec::new(),
            files: vec![(settings_path, settings)],
            agent_session,
        }
    }

    fn signals(&self, payload: &Value) -> Vec<Signal> {
        claude_signals(payload)
    }

    fn usage(&self, payload: &Value) -> Option<Usage> {
        let cost = payload
            .pointer("/cost/total_cost_usd")
            .and_then(|v| v.as_f64());
        let pct = payload
            .pointer("/context_window/used_percentage")
            .and_then(|v| v.as_f64())
            .map(|p| p.round() as u32);
        if cost.is_none() && pct.is_none() {
            return None;
        }
        Some(Usage {
            cost_usd: cost,
            context_pct: pct,
            tokens: None,
        })
    }

    fn startup_markers(&self) -> &'static [Marker] {
        MARKERS
    }

    fn oneshot(&self, model: Option<&str>, prompt: &str) -> OneShotPlan {
        let mut args = vec![
            "claude".into(),
            "-p".into(),
            "--output-format".into(),
            "json".into(),
            "--tools".into(),
            "".into(),
            "--settings".into(),
            "{\"hooks\":{}}".into(),
        ];
        if let Some(m) = model.filter(|m| !m.is_empty()) {
            args.push("--model".into());
            args.push(m.into());
        }
        OneShotPlan {
            args,
            stdin: Some(prompt.to_string()),
        }
    }

    fn oneshot_reply(&self, stdout: &str) -> Option<String> {
        let v: Value = serde_json::from_str(stdout.trim()).ok()?;
        v.get("result")
            .and_then(|r| r.as_str())
            .map(|s| s.to_string())
    }
}

/// Quote a path for embedding inside a JSON command string.
fn shell_quote(path: &str) -> String {
    format!("\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn claude_settings_json(hook_bin: &str) -> String {
    let cmd = format!("{} signal --agent claude", shell_quote(hook_bin));
    let usage = format!("{} usage --agent claude", shell_quote(hook_bin));
    let hook = |matcher: bool| {
        let mut h = json!({
            "hooks": [{ "type": "command", "command": cmd, "timeout": 5 }]
        });
        if matcher {
            h.as_object_mut()
                .unwrap()
                .insert("matcher".into(), json!("*"));
        }
        json!([h])
    };
    let doc = json!({
        "hooks": {
            "SessionStart": hook(false),
            "UserPromptSubmit": hook(false),
            "PreToolUse": hook(true),
            "PostToolUse": hook(true),
            "PermissionRequest": hook(true),
            "Notification": hook(false),
            "Stop": hook(false),
            "SessionEnd": hook(false),
        },
        "statusLine": { "type": "command", "command": usage }
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into())
}

pub fn claude_signals(payload: &Value) -> Vec<Signal> {
    let event = payload
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let cwd = payload.get("cwd").and_then(|v| v.as_str());
    let tool_name = payload
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let tool_input = payload.get("tool_input").cloned().unwrap_or(Value::Null);
    let message = payload
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut out = Vec::new();

    // Always refresh ids when present.
    let agent_session = payload
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let transcript = payload
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if agent_session.is_some() || transcript.is_some() {
        // SessionStarted also flips launching→ready; for other events we still want ids.
        // apply() on SessionStarted when already past launching still stores ids.
        if event == "SessionStart" {
            out.push(Signal::SessionStarted {
                agent_session: agent_session.clone(),
                transcript: transcript.clone(),
            });
        } else if agent_session.is_some() || transcript.is_some() {
            // Store via a no-op-looking SessionStarted only when launching would be wrong.
            // Instead use Other and let the binary merge ids — but apply() only updates ids on
            // SessionStarted when ended, and on SessionStarted always.
            // Spec: "Every payload that carries session_id and transcript_path also refreshes
            // those two fields". The cleanest pure approach: emit SessionStarted for SessionStart
            // only; for other events emit a dedicated path. Looking at apply() — only
            // SessionStarted stores agent_session/transcript for live sessions.
            // So emit SessionStarted solely on SessionStart; binary can set fields for others.
            let _ = (agent_session, transcript);
        }
    }

    match event {
        "SessionStart" => {
            if out.is_empty() {
                out.push(Signal::SessionStarted {
                    agent_session: payload
                        .get("session_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    transcript: payload
                        .get("transcript_path")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                });
            }
        }
        "UserPromptSubmit" => out.push(Signal::PromptSubmitted),
        "PreToolUse" => match tool_name {
            "AskUserQuestion" => {
                let q = first_question_text(&tool_input).unwrap_or_else(|| "Question".into());
                out.push(Signal::QuestionAsked { message: q });
            }
            "ExitPlanMode" => out.push(Signal::ApprovalNeeded {
                message: "Plan is ready for your review".into(),
            }),
            _ => out.push(Signal::ToolStarting {
                name: tool_name.into(),
                detail: tool_detail(tool_name, &tool_input, cwd),
            }),
        },
        "PostToolUse" => out.push(Signal::ToolFinished {
            name: tool_name.into(),
        }),
        "PermissionRequest" => {
            let detail = tool_detail(tool_name, &tool_input, cwd);
            let msg = if detail.is_empty() {
                tool_name.to_string()
            } else {
                format!("{tool_name}: {detail}")
            };
            out.push(Signal::ApprovalNeeded { message: msg });
        }
        "Notification" => {
            let ntype = payload
                .get("notification_type")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match ntype {
                "permission_prompt" => out.push(Signal::ApprovalNeeded {
                    message: if message.is_empty() {
                        "Needs approval".into()
                    } else {
                        message
                    },
                }),
                "elicitation_dialog" => out.push(Signal::QuestionAsked {
                    message: if message.is_empty() {
                        "Has a question".into()
                    } else {
                        message
                    },
                }),
                _ => out.push(Signal::Other),
            }
        }
        "Stop" => out.push(Signal::TurnEnded),
        "SessionEnd" => out.push(Signal::SessionEnded),
        _ => {
            if out.is_empty() {
                out.push(Signal::Other);
            }
        }
    }
    out
}

fn first_question_text(input: &Value) -> Option<String> {
    // AskUserQuestion: questions[0].question or similar.
    if let Some(q) = input
        .pointer("/questions/0/question")
        .and_then(|v| v.as_str())
    {
        return Some(q.to_string());
    }
    if let Some(q) = input.get("question").and_then(|v| v.as_str()) {
        return Some(q.to_string());
    }
    input
        .get("questions")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Refresh session/transcript ids from any Claude hook payload.
pub fn claude_ids(payload: &Value) -> (Option<String>, Option<String>) {
    (
        payload
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        payload
            .get("transcript_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req<'a>(
        resume: bool,
        model: Option<&'a str>,
        autonomy: bool,
        prompt: Option<&'a str>,
        agent_session: Option<&'a str>,
    ) -> LaunchRequest<'a> {
        LaunchRequest {
            session_id: "11111111-1111-4111-8111-111111111111",
            agent_session,
            resume,
            model,
            autonomy,
            prompt,
            hook_bin: "/opt/My Apps/syncaidync",
            run_dir: "/tmp/run/s1",
        }
    }

    #[test]
    fn oneshot_args_and_reply() {
        let a = ClaudeAdapter;
        let plan = a.oneshot(Some("sonnet"), "hello");
        assert!(plan.args.contains(&"-p".into()));
        assert!(plan.args.contains(&"--tools".into()));
        assert_eq!(plan.stdin.as_deref(), Some("hello"));
        let reply = a.oneshot_reply(r#"{"result":"plan text"}"#).unwrap();
        assert_eq!(reply, "plan text");
    }

    #[test]
    fn launch_start_and_resume() {
        let a = ClaudeAdapter;
        let start = a.launch(&req(false, Some("sonnet"), true, Some("hi"), None));
        assert_eq!(start.args[0], "claude");
        assert!(start.args.contains(&"--session-id".into()));
        assert!(start
            .args
            .contains(&"--dangerously-skip-permissions".into()));
        assert!(start.args.contains(&"hi".into()));
        assert_eq!(
            start.agent_session.as_deref(),
            Some("11111111-1111-4111-8111-111111111111")
        );
        assert_eq!(start.files.len(), 1);

        let resume = a.launch(&req(
            true,
            Some("opus"),
            false,
            Some("ignored"),
            Some("agent-xyz"),
        ));
        assert!(resume.args.contains(&"--resume".into()));
        assert!(resume.args.contains(&"agent-xyz".into()));
        assert!(!resume.args.iter().any(|a| a == "ignored"));
        assert!(resume.agent_session.is_none());
    }

    #[test]
    fn settings_file_has_eight_events_and_quotes_spaces() {
        let raw = claude_settings_json("/opt/My Apps/syncaidync");
        let v: Value = serde_json::from_str(&raw).unwrap();
        let hooks = v.get("hooks").unwrap().as_object().unwrap();
        for name in [
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "PermissionRequest",
            "Notification",
            "Stop",
            "SessionEnd",
        ] {
            assert!(hooks.contains_key(name), "missing {name}");
        }
        let cmd = hooks["SessionStart"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(cmd.contains("\"/opt/My Apps/syncaidync\""));
        assert!(v.pointer("/statusLine/command").is_some());
    }

    #[test]
    fn signal_mapping_fixtures() {
        let a = ClaudeAdapter;
        assert_eq!(
            a.signals(&json!({
                "hook_event_name": "SessionStart",
                "session_id": "abc",
                "transcript_path": "/t.jsonl"
            })),
            vec![Signal::SessionStarted {
                agent_session: Some("abc".into()),
                transcript: Some("/t.jsonl".into()),
            }]
        );
        assert_eq!(
            a.signals(&json!({"hook_event_name": "UserPromptSubmit"})),
            vec![Signal::PromptSubmitted]
        );
        assert_eq!(
            a.signals(&json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": {"command": "rm -rf build"},
                "cwd": "/ws"
            })),
            vec![Signal::ToolStarting {
                name: "Bash".into(),
                detail: "rm -rf build".into(),
            }]
        );
        assert_eq!(
            a.signals(&json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "AskUserQuestion",
                "tool_input": {"questions": [{"question": "Which DB?"}]}
            })),
            vec![Signal::QuestionAsked {
                message: "Which DB?".into()
            }]
        );
        assert_eq!(
            a.signals(&json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "ExitPlanMode",
                "tool_input": {}
            })),
            vec![Signal::ApprovalNeeded {
                message: "Plan is ready for your review".into()
            }]
        );
        assert_eq!(
            a.signals(&json!({
                "hook_event_name": "PermissionRequest",
                "tool_name": "Bash",
                "tool_input": {"command": "ls"}
            })),
            vec![Signal::ApprovalNeeded {
                message: "Bash: ls".into()
            }]
        );
        assert_eq!(
            a.signals(&json!({
                "hook_event_name": "Notification",
                "notification_type": "permission_prompt",
                "message": "Allow Bash?"
            })),
            vec![Signal::ApprovalNeeded {
                message: "Allow Bash?".into()
            }]
        );
        assert_eq!(
            a.signals(&json!({
                "hook_event_name": "Notification",
                "notification_type": "idle_prompt"
            })),
            vec![Signal::Other]
        );
        assert_eq!(
            a.signals(&json!({"hook_event_name": "Stop"})),
            vec![Signal::TurnEnded]
        );
        assert_eq!(
            a.signals(&json!({"hook_event_name": "SessionEnd"})),
            vec![Signal::SessionEnded]
        );
        assert_eq!(
            a.signals(&json!({"hook_event_name": "PostToolUse", "tool_name": "Bash"})),
            vec![Signal::ToolFinished {
                name: "Bash".into()
            }]
        );
    }

    #[test]
    fn usage_parsing() {
        let a = ClaudeAdapter;
        let u = a
            .usage(&json!({
                "cost": {"total_cost_usd": 0.42},
                "context_window": {"used_percentage": 31.4}
            }))
            .unwrap();
        assert_eq!(u.cost_usd, Some(0.42));
        assert_eq!(u.context_pct, Some(31));
        assert!(a.usage(&json!({})).is_none());
    }
}
