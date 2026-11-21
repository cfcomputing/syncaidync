//! Cursor Agent adapter. Observes by activity.

use serde_json::Value;

use crate::model::Usage;
use crate::signal::Signal;
use crate::startup::Marker;

use super::{Adapter, AgentId, LaunchPlan, LaunchRequest, Observes, OneShotPlan};

const MARKERS: &[Marker] = &[
    Marker {
        needle: "workspace trust",
        message: "Asking whether to trust this folder",
    },
    Marker {
        needle: "log in",
        message: "Needs you to sign in",
    },
];

#[derive(Debug, Default, Clone, Copy)]
pub struct CursorAdapter;

impl Adapter for CursorAdapter {
    fn id(&self) -> AgentId {
        AgentId::Cursor
    }

    fn label(&self) -> &'static str {
        "Cursor Agent"
    }

    fn bin_names(&self) -> &'static [&'static str] {
        &["agent", "cursor-agent"]
    }

    fn observes(&self) -> Observes {
        Observes::Activity
    }

    fn models(&self) -> &'static [&'static str] {
        &[]
    }

    fn launch(&self, req: &LaunchRequest<'_>) -> LaunchPlan {
        let mut args = vec!["agent".into()];
        if req.resume {
            if let Some(id) = req.agent_session {
                args.push("--resume".into());
                args.push(id.into());
            } else {
                args.push("--continue".into());
            }
        }
        if let Some(model) = req.model.filter(|m| !m.is_empty()) {
            args.push("--model".into());
            args.push(model.into());
        }
        if req.autonomy {
            args.push("--force".into());
        }
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

    fn signals(&self, _payload: &Value) -> Vec<Signal> {
        Vec::new()
    }

    fn usage(&self, _payload: &Value) -> Option<Usage> {
        // Interactive Cursor Agent has no hook that reports tokens or cost. Print-mode
        // stream-json carries token counts only, and we do not write project hook files.
        None
    }

    fn startup_markers(&self) -> &'static [Marker] {
        MARKERS
    }

    fn oneshot(&self, model: Option<&str>, prompt: &str) -> OneShotPlan {
        let mut args = vec![
            "agent".into(),
            "--print".into(),
            "--mode".into(),
            "ask".into(),
            "--output-format".into(),
            "text".into(),
        ];
        if let Some(m) = model.filter(|m| !m.is_empty()) {
            args.push("--model".into());
            args.push(m.into());
        }
        args.push(prompt.into());
        OneShotPlan { args, stdin: None }
    }

    fn oneshot_reply(&self, stdout: &str) -> Option<String> {
        let t = stdout.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oneshot_ask_mode() {
        let a = CursorAdapter;
        let plan = a.oneshot(None, "q");
        assert!(plan.args.contains(&"--mode".into()));
        assert!(plan.args.contains(&"ask".into()));
        assert_eq!(plan.args.last().map(String::as_str), Some("q"));
        assert_eq!(a.oneshot_reply("plain").as_deref(), Some("plain"));
    }

    #[test]
    fn launch_start_resume() {
        let a = CursorAdapter;
        let start = a.launch(&LaunchRequest {
            session_id: "s",
            agent_session: None,
            resume: false,
            model: Some("gpt"),
            autonomy: true,
            prompt: Some("hi"),
            hook_bin: "/bin/x",
            run_dir: "/tmp",
        });
        assert!(start.args.contains(&"--force".into()));
        assert!(start.args.contains(&"hi".into()));

        let resume = a.launch(&LaunchRequest {
            session_id: "s",
            agent_session: None,
            resume: true,
            model: None,
            autonomy: false,
            prompt: Some("x"),
            hook_bin: "/bin/x",
            run_dir: "/tmp",
        });
        assert!(resume.args.contains(&"--continue".into()));
        assert!(!resume.args.iter().any(|a| a == "x"));
    }
}
