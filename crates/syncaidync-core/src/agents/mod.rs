//! Agent adapters: describe how to start, observe, and ask each CLI.

mod activity;
mod claude;
mod codex;
mod cursor;
mod detail;
mod pi;
mod shell;

pub use activity::{ActivityEvent, ActivityWatcher};
pub use claude::{claude_ids, claude_settings_json, claude_signals, ClaudeAdapter};
pub use codex::{codex_signals, CodexAdapter};
pub use cursor::CursorAdapter;
pub use detail::tool_detail;
pub use pi::PiAdapter;
pub use shell::{shell_question, ShellAdapter};

use serde::{Deserialize, Serialize};

use crate::model::Usage;
use crate::signal::Signal;
use crate::startup::Marker;

/// Which agent CLI a session runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentId {
    Claude,
    Codex,
    Pi,
    Cursor,
    Shell,
}

impl AgentId {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentId::Claude => "claude",
            AgentId::Codex => "codex",
            AgentId::Pi => "pi",
            AgentId::Cursor => "cursor",
            AgentId::Shell => "shell",
        }
    }
}

/// How SyncAiDync learns what the agent is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Observes {
    Signals,
    Activity,
}

/// What the binary needs to start or resume an agent process.
#[derive(Debug, Clone)]
pub struct LaunchRequest<'a> {
    pub session_id: &'a str,
    pub agent_session: Option<&'a str>,
    pub resume: bool,
    pub model: Option<&'a str>,
    pub autonomy: bool,
    pub prompt: Option<&'a str>,
    pub hook_bin: &'a str,
    pub run_dir: &'a str,
}

/// Arguments, environment, and files to write before spawning the agent.
#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub files: Vec<(String, String)>,
    pub agent_session: Option<String>,
}

/// How to run the agent once for a plan or takeaways.
#[derive(Debug, Clone)]
pub struct OneShotPlan {
    pub args: Vec<String>,
    pub stdin: Option<String>,
}

/// Describes one agent CLI. The core crate only describes; the binary performs I/O.
pub trait Adapter {
    fn id(&self) -> AgentId;
    fn label(&self) -> &'static str;
    fn bin_names(&self) -> &'static [&'static str];
    fn observes(&self) -> Observes;
    fn models(&self) -> &'static [&'static str];
    fn launch(&self, req: &LaunchRequest<'_>) -> LaunchPlan;
    fn signals(&self, payload: &serde_json::Value) -> Vec<Signal>;
    fn usage(&self, payload: &serde_json::Value) -> Option<Usage>;
    fn startup_markers(&self) -> &'static [Marker];
    fn oneshot(&self, model: Option<&str>, prompt: &str) -> OneShotPlan;
    fn oneshot_reply(&self, stdout: &str) -> Option<String>;
}

const CLAUDE: ClaudeAdapter = ClaudeAdapter;
const CODEX: CodexAdapter = CodexAdapter;
const PI: PiAdapter = PiAdapter;
const CURSOR: CursorAdapter = CursorAdapter;

/// Look up a static adapter by id (shell needs a path — use ShellAdapter::new separately).
pub fn adapter_for(id: AgentId) -> Option<&'static dyn Adapter> {
    match id {
        AgentId::Claude => Some(&CLAUDE),
        AgentId::Codex => Some(&CODEX),
        AgentId::Pi => Some(&PI),
        AgentId::Cursor => Some(&CURSOR),
        AgentId::Shell => None,
    }
}
