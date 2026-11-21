//! Session and related data shapes.

use serde::{Deserialize, Serialize};

use crate::agents::AgentId;
use crate::goal::Goal;

/// Lifecycle state of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Launching,
    Working,
    Blocked,
    Ready,
    Ended,
}

/// Why a session is blocked on the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockedKind {
    Approval,
    Question,
    Startup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blocked {
    pub kind: BlockedKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceKind {
    Worktree,
    Folder,
    Scratch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub kind: WorkspaceKind,
    pub path: String,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub base_commit: Option<String>,
    pub owned: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub context_pct: Option<u32>,
    /// Cumulative tokens reported for the session (Codex, Pi, …). Cost stays preferred when both exist.
    #[serde(default)]
    pub tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolUse {
    pub name: String,
    pub detail: String,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub agent: AgentId,
    #[serde(default)]
    pub model: Option<String>,
    pub autonomy: bool,
    #[serde(default)]
    pub prompt: Option<String>,
    pub state: State,
    #[serde(default)]
    pub blocked: Option<Blocked>,
    pub snoozed: bool,
    #[serde(default)]
    pub archived: bool,
    pub live: bool,
    #[serde(default)]
    pub exit_code: Option<i32>,
    pub workspace: Workspace,
    pub created_at: u64,
    pub updated_at: u64,
    pub state_since: u64,
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(default)]
    pub tools: Vec<ToolUse>,
    #[serde(default)]
    pub agent_session: Option<String>,
    #[serde(default)]
    pub transcript: Option<String>,
    #[serde(default)]
    pub pull: Option<String>,
    #[serde(default)]
    pub goal: Option<Goal>,
}

#[derive(Debug, Clone)]
pub struct NewSession {
    pub id: String,
    pub agent: AgentId,
    pub model: Option<String>,
    pub autonomy: bool,
    pub prompt: Option<String>,
    pub title: Option<String>,
    pub workspace: Workspace,
    pub pull: Option<String>,
    pub now_ms: u64,
}

impl Session {
    pub fn new(args: NewSession) -> Self {
        let title = args
            .title
            .filter(|t| !t.trim().is_empty())
            .map(|t| truncate_chars(t.trim(), 120))
            .unwrap_or_else(|| default_title(args.prompt.as_deref()));
        Self {
            id: args.id,
            title,
            agent: args.agent,
            model: args.model,
            autonomy: args.autonomy,
            prompt: args.prompt,
            state: State::Launching,
            blocked: None,
            snoozed: false,
            archived: false,
            live: true,
            exit_code: None,
            workspace: args.workspace,
            created_at: args.now_ms,
            updated_at: args.now_ms,
            state_since: args.now_ms,
            usage: None,
            tools: Vec::new(),
            agent_session: None,
            transcript: None,
            pull: args.pull,
            goal: None,
        }
    }
}

/// Put the session into `launching` when starting or resuming a process.
pub fn mark_launching(session: &mut Session, now_ms: u64) {
    set_state(session, State::Launching, None, now_ms);
    session.live = true;
    session.exit_code = None;
    session.updated_at = now_ms;
}

/// Mark the process gone. Always clears `blocked`.
pub fn mark_ended(session: &mut Session, exit_code: Option<i32>, now_ms: u64) {
    set_state(session, State::Ended, None, now_ms);
    session.live = false;
    session.exit_code = exit_code;
    session.updated_at = now_ms;
}

/// Block on a startup prompt (trust folder, sign in, …).
pub fn mark_startup_blocked(session: &mut Session, message: impl Into<String>, now_ms: u64) {
    if session.state == State::Ended {
        return;
    }
    set_state(
        session,
        State::Blocked,
        Some(Blocked {
            kind: BlockedKind::Startup,
            message: message.into(),
        }),
        now_ms,
    );
    session.updated_at = now_ms;
}

/// Append a tool use, keeping the newest 20. `detail` is cut to one line, 160 characters.
pub fn push_tool(
    session: &mut Session,
    name: impl Into<String>,
    detail: impl Into<String>,
    at: u64,
) {
    let detail = one_line_cut(&detail.into(), 160);
    session.tools.push(ToolUse {
        name: name.into(),
        detail,
        at,
    });
    if session.tools.len() > 20 {
        let drop = session.tools.len() - 20;
        session.tools.drain(0..drop);
    }
}

/// Trim and set the title. Empty or longer than 120 characters (after trim) is an error.
pub fn set_title(session: &mut Session, title: &str, now_ms: u64) -> Result<(), &'static str> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 120 {
        return Err("Title must be between 1 and 120 characters.");
    }
    session.title = title.to_string();
    session.updated_at = now_ms;
    Ok(())
}

/// First non-empty line of the prompt, cut to 60 characters; else "Untitled session".
pub fn default_title(prompt: Option<&str>) -> String {
    let Some(prompt) = prompt else {
        return "Untitled session".to_string();
    };
    for line in prompt.lines() {
        let line = line.trim();
        if !line.is_empty() {
            return truncate_chars(line, 60);
        }
    }
    "Untitled session".to_string()
}

pub(crate) fn set_state(
    session: &mut Session,
    state: State,
    blocked: Option<Blocked>,
    now_ms: u64,
) {
    let kind_changed = match (&session.blocked, &blocked) {
        (Some(a), Some(b)) => a.kind != b.kind,
        (None, None) => false,
        _ => true,
    };
    let state_changed = session.state != state;
    session.state = state;
    session.blocked = blocked;
    if state_changed || kind_changed {
        session.state_since = now_ms;
    }
}

fn one_line_cut(s: &str, max_chars: usize) -> String {
    let line = s.lines().next().unwrap_or(s).trim_end();
    truncate_chars(line, max_chars)
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(now: u64) -> Session {
        Session::new(NewSession {
            id: "s1".into(),
            agent: AgentId::Shell,
            model: None,
            autonomy: false,
            prompt: Some("hello\nworld".into()),
            title: None,
            workspace: Workspace {
                kind: WorkspaceKind::Scratch,
                path: "/tmp/x".into(),
                repo: None,
                branch: None,
                base: None,
                base_commit: None,
                owned: true,
            },
            pull: None,
            now_ms: now,
        })
    }

    #[test]
    fn default_title_from_first_line() {
        assert_eq!(default_title(Some("  Fix login  \nmore")), "Fix login");
        assert_eq!(default_title(Some("\n\n")), "Untitled session");
        assert_eq!(default_title(None), "Untitled session");
        let long = "a".repeat(80);
        assert_eq!(default_title(Some(&long)).chars().count(), 60);
    }

    #[test]
    fn set_title_bounds() {
        let mut s = scratch(1);
        assert!(set_title(&mut s, "  ok  ", 2).is_ok());
        assert_eq!(s.title, "ok");
        assert!(set_title(&mut s, "", 3).is_err());
        assert!(set_title(&mut s, &"x".repeat(121), 3).is_err());
    }

    #[test]
    fn push_tool_caps_and_cuts() {
        let mut s = scratch(1);
        for i in 0..25 {
            push_tool(&mut s, "T", format!("line1\nline2-{i}"), i);
        }
        assert_eq!(s.tools.len(), 20);
        assert_eq!(s.tools[0].at, 5);
        assert_eq!(s.tools.last().unwrap().detail, "line1");
        let long = "字".repeat(200);
        push_tool(&mut s, "T", &long, 100);
        assert_eq!(s.tools.last().unwrap().detail.chars().count(), 160);
    }

    #[test]
    fn session_json_round_trip() {
        let mut s = scratch(1_795_000_000_000);
        s.model = Some("opus".into());
        s.usage = Some(Usage {
            cost_usd: Some(0.42),
            context_pct: Some(31),
            tokens: None,
        });
        s.agent_session = Some("abc".into());
        let json = serde_json::to_value(&s).unwrap();
        let keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        for expected in [
            "id",
            "title",
            "agent",
            "model",
            "autonomy",
            "prompt",
            "state",
            "blocked",
            "snoozed",
            "archived",
            "live",
            "exit_code",
            "workspace",
            "created_at",
            "updated_at",
            "state_since",
            "usage",
            "tools",
            "agent_session",
            "transcript",
            "pull",
            "goal",
        ] {
            assert!(keys.iter().any(|k| k == expected), "missing {expected}");
        }
        assert_eq!(keys.len(), 22);
        assert_eq!(json["agent"], "shell");
        assert_eq!(json["state"], "launching");
        assert_eq!(json["title"], "hello");
        let back: Session = serde_json::from_value(json).unwrap();
        assert_eq!(back.id, s.id);
        assert_eq!(back.usage.as_ref().unwrap().context_pct, Some(31));
    }

    #[test]
    fn serde_ignores_unknown_and_defaults_missing() {
        let v = serde_json::json!({
            "id": "x",
            "title": "t",
            "agent": "claude",
            "autonomy": false,
            "state": "ready",
            "snoozed": false,
            "live": false,
            "workspace": {
                "kind": "folder",
                "path": "/a",
                "owned": false
            },
            "created_at": 1,
            "updated_at": 1,
            "state_since": 1,
            "extra_future_field": true
        });
        let s: Session = serde_json::from_value(v).unwrap();
        assert!(!s.archived);
        assert!(s.model.is_none());
        assert!(s.tools.is_empty());
        assert!(s.blocked.is_none());
        assert_eq!(s.agent, AgentId::Claude);
    }
}
