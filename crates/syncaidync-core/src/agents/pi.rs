//! Pi adapter. Observes by activity; a small `-e` extension reports tokens.

use serde_json::Value;

use crate::model::Usage;
use crate::signal::Signal;
use crate::startup::Marker;

use super::{Adapter, AgentId, LaunchPlan, LaunchRequest, Observes, OneShotPlan};

const MARKERS: &[Marker] = &[Marker {
    needle: "api key",
    message: "Needs credentials",
}];

/// Extension loaded with `-e`. Posts each assistant message's `usage.totalTokens` to SyncAiDync.
const USAGE_EXTENSION: &str = r#"/** SyncAiDync: report Pi token usage. */
export default function (pi) {
  const base = process.env.SYNCAIDYNC_URL;
  const session = process.env.SYNCAIDYNC_SESSION;
  const token = process.env.SYNCAIDYNC_SIGNAL_TOKEN;
  if (!base || !session || !token) return;

  pi.on("message_end", async (event) => {
    if (event.message?.role !== "assistant") return;
    const tokens = event.message.usage?.totalTokens;
    if (typeof tokens !== "number" || !(tokens > 0)) return;
    try {
      await fetch(`${base}/agent/v1/usage?session=${encodeURIComponent(session)}`, {
        method: "POST",
        headers: {
          Authorization: `Bearer ${token}`,
          "Content-Type": "application/json",
        },
        body: JSON.stringify({ agent: "pi", payload: { tokens } }),
      });
    } catch {
      // Quiet: same as `syncaidync usage`.
    }
  });
}
"#;

#[derive(Debug, Default, Clone, Copy)]
pub struct PiAdapter;

impl Adapter for PiAdapter {
    fn id(&self) -> AgentId {
        AgentId::Pi
    }

    fn label(&self) -> &'static str {
        "Pi"
    }

    fn bin_names(&self) -> &'static [&'static str] {
        &["pi"]
    }

    fn observes(&self) -> Observes {
        Observes::Activity
    }

    fn models(&self) -> &'static [&'static str] {
        &[]
    }

    fn launch(&self, req: &LaunchRequest<'_>) -> LaunchPlan {
        // Same command for start and resume; Pi creates the id if missing.
        let id = req.agent_session.unwrap_or(req.session_id).to_string();
        let ext_path = format!("{}/pi-usage.js", req.run_dir.trim_end_matches('/'));
        let mut args = vec![
            "pi".into(),
            "--session-id".into(),
            id.clone(),
            "--extension".into(),
            ext_path.clone(),
        ];
        if let Some(model) = req.model.filter(|m| !m.is_empty()) {
            args.push("--model".into());
            args.push(model.into());
        }
        if req.autonomy {
            // Verified 2026-11-17: --approve trusts project-local files for this run,
            // not a full permission bypass. Kept as the closest documented autonomy flag.
            args.push("--approve".into());
        }
        if !req.resume {
            if let Some(prompt) = req.prompt.filter(|p| !p.is_empty()) {
                args.push(prompt.into());
            }
        }
        LaunchPlan {
            args,
            env: Vec::new(),
            files: vec![(ext_path, USAGE_EXTENSION.into())],
            agent_session: Some(id),
        }
    }

    fn signals(&self, _payload: &Value) -> Vec<Signal> {
        Vec::new()
    }

    fn usage(&self, payload: &Value) -> Option<Usage> {
        pi_usage(payload)
    }

    fn startup_markers(&self) -> &'static [Marker] {
        MARKERS
    }

    fn oneshot(&self, model: Option<&str>, prompt: &str) -> OneShotPlan {
        let mut args = vec![
            "pi".into(),
            "--print".into(),
            "--no-session".into(),
            "--no-tools".into(),
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

fn json_u64(v: &Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_i64().and_then(|n| u64::try_from(n).ok()))
        .or_else(|| v.as_f64().and_then(|n| (n >= 0.0).then_some(n as u64)))
}

pub fn pi_usage(payload: &Value) -> Option<Usage> {
    let tokens = payload
        .get("tokens")
        .and_then(json_u64)
        .filter(|n| *n > 0)?;
    Some(Usage {
        cost_usd: None,
        context_pct: None,
        tokens: Some(tokens),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oneshot_prompt_as_arg() {
        let a = PiAdapter;
        let plan = a.oneshot(Some("m"), "prompt text");
        assert!(plan.args.contains(&"--print".into()));
        assert!(plan.args.contains(&"--no-tools".into()));
        assert!(plan.stdin.is_none());
        assert_eq!(plan.args.last().map(String::as_str), Some("prompt text"));
        assert_eq!(a.oneshot_reply("  out  ").as_deref(), Some("out"));
    }

    #[test]
    fn launch_start_resume() {
        let a = PiAdapter;
        let start = a.launch(&LaunchRequest {
            session_id: "sid",
            agent_session: None,
            resume: false,
            model: Some("gpt"),
            autonomy: true,
            prompt: Some("hello"),
            hook_bin: "/bin/x",
            run_dir: "/tmp/run",
        });
        assert!(start.args.contains(&"--session-id".into()));
        assert!(start.args.contains(&"sid".into()));
        assert!(start.args.contains(&"--approve".into()));
        assert!(start.args.contains(&"hello".into()));
        assert!(start.args.contains(&"--extension".into()));
        assert!(start.args.iter().any(|a| a.ends_with("pi-usage.js")));
        assert_eq!(start.files.len(), 1);
        assert!(start.files[0].1.contains("totalTokens"));
        assert_eq!(start.agent_session.as_deref(), Some("sid"));

        let resume = a.launch(&LaunchRequest {
            session_id: "sid",
            agent_session: Some("old"),
            resume: true,
            model: None,
            autonomy: false,
            prompt: Some("no"),
            hook_bin: "/bin/x",
            run_dir: "/tmp/run",
        });
        assert!(resume.args.contains(&"old".into()));
        assert!(!resume.args.iter().any(|a| a == "no"));
        assert!(resume.args.contains(&"--extension".into()));
    }

    #[test]
    fn usage_tokens() {
        let a = PiAdapter;
        assert_eq!(
            a.usage(&serde_json::json!({ "tokens": 1200 }))
                .unwrap()
                .tokens,
            Some(1200)
        );
        assert!(a.usage(&serde_json::json!({})).is_none());
        assert!(a.usage(&serde_json::json!({ "tokens": 0 })).is_none());
    }
}
