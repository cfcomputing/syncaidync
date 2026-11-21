//! In-memory session registry with persistence and terminal processes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use serde::{Deserialize, Serialize};
use syncaidync_core::{
    adapter_for, apply, find_marker, mark_ended, mark_launching, mark_startup_blocked,
    normalize_tail, set_title, shell_question, ActivityEvent, ActivityWatcher, Adapter, AgentId,
    BlockedKind, ClaudeAdapter, NewSession, Observes, Session, ShellAdapter, Signal, State, Usage,
    Workspace,
};
use tokio::sync::Mutex;

use crate::agents_discover;
use crate::http::sse::Hub;
use crate::ids::{random_hex, uuid_v4};
use crate::pty::{send_text, InputHook, OutputHook, SpawnOpts, Terminal};
use crate::store::{self, Saver};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionsFile {
    version: u32,
    sessions: Vec<Session>,
}

pub struct Registry {
    me: Weak<Registry>,
    inner: Arc<Mutex<Inner>>,
    lifecycle: Mutex<()>,
    pub(crate) goal_git_write: Mutex<()>,
    hub: Hub,
    home: PathBuf,
    port: u16,
    saver: Saver<SessionsFile>,
    dev_shell: bool,
}

struct Inner {
    sessions: HashMap<String, Session>,
    terminals: HashMap<String, Arc<Terminal>>,
    signal_tokens: HashMap<String, String>,
    activity: HashMap<String, ActivityWatcher>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSession {
    pub agent: String,
    pub model: Option<String>,
    pub title: Option<String>,
    pub prompt: Option<String>,
    #[serde(default)]
    pub autonomy: bool,
    pub workspace: CreateWorkspace,
    #[serde(default)]
    pub goal: Option<syncaidync_core::NewGoal>,
}

/// Inputs for starting a session on an existing workspace.
pub struct PreparedSession {
    pub agent: AgentId,
    pub model: Option<String>,
    pub autonomy: bool,
    pub title: Option<String>,
    pub prompt: Option<String>,
    pub workspace: Workspace,
    pub pull: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CreateWorkspace {
    Worktree {
        repo: String,
        base: String,
        branch: String,
    },
    Folder {
        path: String,
    },
    Scratch {},
}

impl Registry {
    pub async fn load(home: PathBuf, hub: Hub, port: u16, dev_shell: bool) -> Arc<Self> {
        let path = home.join("sessions.json");
        let mut sessions = HashMap::new();
        if let Ok(Some(file)) = store::load::<SessionsFile>(&path) {
            for mut s in file.sessions {
                // A session waiting on its baseline when the server stopped never started.
                let stalled = !s.live && s.state == State::Launching;
                if s.live || stalled {
                    mark_ended(&mut s, None, now_ms());
                }
                if let Some(g) = s.goal.as_mut() {
                    syncaidync_core::goal::pause_for_end(g);
                }
                sessions.insert(s.id.clone(), s);
            }
        }
        let saver = Saver::new(path);
        let reg = Arc::new_cyclic(|me| Self {
            me: me.clone(),
            inner: Arc::new(Mutex::new(Inner {
                sessions,
                terminals: HashMap::new(),
                signal_tokens: HashMap::new(),
                activity: HashMap::new(),
            })),
            lifecycle: Mutex::new(()),
            goal_git_write: Mutex::new(()),
            hub,
            home,
            port,
            saver,
            dev_shell,
        });
        reg.persist().await;
        reg.spawn_goal_ticker();
        reg
    }

    /// Every few seconds, pause goals whose agent passed a cost or time limit mid-turn.
    fn spawn_goal_ticker(&self) {
        let weak = self.me.clone();
        let every = std::env::var("SYNCAIDYNC_GOAL_TICK_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(15_000)
            .max(50);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(every));
            loop {
                interval.tick().await;
                let Some(reg) = weak.upgrade() else { break };
                let ids: Vec<String> = {
                    let g = reg.inner.lock().await;
                    g.sessions
                        .values()
                        .filter(|s| {
                            s.state == State::Working
                                && s.goal.as_ref().is_some_and(|g| {
                                    g.status == syncaidync_core::GoalStatus::Working
                                })
                        })
                        .map(|s| s.id.clone())
                        .collect()
                };
                for id in ids {
                    crate::goals::enforce_limits(reg.clone(), id).await;
                }
            }
        });
    }

    /// Change a session's goal (or anything else on it) under the lock, then publish and save
    /// when the goal changed.
    pub async fn update_goal<T>(
        &self,
        id: &str,
        f: impl FnOnce(&mut Session) -> Result<T, CreateError>,
    ) -> Result<(Session, T), CreateError> {
        let mut g = self.inner.lock().await;
        let session = g
            .sessions
            .get_mut(id)
            .ok_or_else(|| CreateError::not_found("No session with that id."))?;
        let before = session.goal.clone();
        let out = f(session)?;
        let changed = session.goal != before;
        if changed {
            session.updated_at = now_ms();
        }
        let snapshot = session.clone();
        drop(g);
        if changed {
            self.publish_session(&snapshot).await;
            self.persist().await;
        }
        Ok((snapshot, out))
    }

    pub async fn set_goal_running(&self, id: &str, check: Option<String>) {
        let _ = self
            .update_goal(id, |s| {
                if let Some(g) = s.goal.as_mut() {
                    g.running = check;
                }
                Ok(())
            })
            .await;
    }

    /// Start the agent of a goal session once its baseline has run.
    pub async fn start_after_baseline(self: &Arc<Self>, id: &str) -> Result<(), CreateError> {
        let _lifecycle = self.lifecycle.lock().await;
        let mut session = self
            .get(id)
            .await
            .ok_or_else(|| CreateError::not_found("No session with that id."))?;
        if session.live {
            return Ok(());
        }
        mark_launching(&mut session, now_ms());
        self.start_process(&mut session).await?;
        self.persist().await;
        Ok(())
    }

    fn spawn_after_turn(&self, id: &str) {
        if let Some(reg) = self.me.upgrade() {
            let id = id.to_string();
            tokio::spawn(async move {
                let _ = crate::goals::run_round(reg, id, crate::goals::Trigger::Turn).await;
            });
        }
    }

    pub fn home_path(&self) -> PathBuf {
        self.home.clone()
    }

    pub async fn list(&self) -> Vec<Session> {
        let g = self.inner.lock().await;
        let mut v: Vec<_> = g.sessions.values().cloned().collect();
        v.sort_by_key(|s| s.created_at);
        v
    }

    pub async fn get(&self, id: &str) -> Option<Session> {
        self.inner.lock().await.sessions.get(id).cloned()
    }

    pub async fn signal_token(&self, id: &str) -> Option<String> {
        self.inner.lock().await.signal_tokens.get(id).cloned()
    }

    pub async fn terminal(&self, id: &str) -> Option<Arc<Terminal>> {
        self.inner.lock().await.terminals.get(id).cloned()
    }

    /// PTY leader pids for every live session (each is also its process group id).
    pub async fn live_session_pids(&self) -> Vec<u32> {
        let g = self.inner.lock().await;
        g.terminals.values().filter_map(|t| t.pid).collect()
    }

    pub async fn create(self: &Arc<Self>, req: CreateSession) -> Result<Session, CreateError> {
        let _lifecycle = self.lifecycle.lock().await;
        let agent = parse_agent(&req.agent)?;
        self.ensure_agent(agent)?;

        let id = uuid_v4();
        let now = now_ms();
        let goal = match req.goal {
            Some(new) => Some(
                syncaidync_core::goal::validate_new(new, now, 0.0)
                    .map_err(CreateError::unprocessable)?,
            ),
            None => None,
        };
        let workspace = self.build_workspace(&id, req.workspace).await?;
        let mut session = Session::new(NewSession {
            id: id.clone(),
            agent,
            model: req.model,
            autonomy: req.autonomy,
            prompt: req.prompt.clone(),
            title: req.title,
            workspace,
            pull: None,
            now_ms: now,
        });

        if let Some(goal) = goal {
            crate::goals::record_start_hashes(
                &self.home,
                &id,
                &goal,
                Path::new(&session.workspace.path),
            )
            .await
            .map_err(CreateError::unprocessable)?;
            let baseline = goal.status == syncaidync_core::GoalStatus::Baseline;
            session.goal = Some(goal);
            if baseline {
                // The agent starts once the checks have run once.
                session.live = false;
                {
                    let mut g = self.inner.lock().await;
                    g.sessions.insert(id.clone(), session.clone());
                }
                self.hub.publish("session", &session);
                self.persist().await;
                if let Some(reg) = self.me.upgrade() {
                    tokio::spawn(crate::goals::baseline_then_start(reg, id.clone()));
                }
                return Ok(session);
            }
        }

        self.start_process(&mut session).await?;
        self.persist().await;
        Ok(session)
    }

    /// Start a session on an already-built workspace (e.g. a pull-request worktree).
    pub async fn create_prepared(
        self: &Arc<Self>,
        args: PreparedSession,
    ) -> Result<Session, CreateError> {
        self.ensure_agent(args.agent)?;
        let id = uuid_v4();
        let now = now_ms();
        let mut session = Session::new(NewSession {
            id: id.clone(),
            agent: args.agent,
            model: args.model,
            autonomy: args.autonomy,
            prompt: args.prompt,
            title: args.title,
            workspace: args.workspace,
            pull: args.pull,
            now_ms: now,
        });
        self.start_process(&mut session).await?;
        self.persist().await;
        Ok(session)
    }

    fn ensure_agent(&self, agent: AgentId) -> Result<(), CreateError> {
        if agent == AgentId::Shell && !self.dev_shell {
            return Err(CreateError::unprocessable(
                "The shell agent is only available with SYNCAIDYNC_DEV_SHELL=1.",
            ));
        }
        if agent != AgentId::Shell && agents_discover::find_bin(agent).is_none() {
            return Err(CreateError::unprocessable(format!(
                "The {} CLI is not installed on this machine.",
                agent.as_str()
            )));
        }
        Ok(())
    }

    pub async fn import_history(
        &self,
        item: crate::discovery::HistoryItem,
    ) -> Result<Session, CreateError> {
        let workspace = crate::workspaces::create_folder(&item.cwd)
            .await
            .map_err(CreateError::from_workspace)?;
        let mut g = self.inner.lock().await;
        if g.sessions.values().any(|s| {
            s.agent == item.agent && s.agent_session.as_deref() == Some(&item.agent_session)
        }) {
            return Err(CreateError::conflict(
                "That session has already been imported.",
            ));
        }
        let mut session = Session::new(NewSession {
            id: uuid_v4(),
            agent: item.agent,
            model: None,
            autonomy: false,
            prompt: Some(item.first_prompt),
            title: None,
            workspace,
            pull: None,
            now_ms: item.last_active,
        });
        mark_ended(&mut session, None, item.last_active);
        session.agent_session = Some(item.agent_session);
        session.transcript = Some(item.transcript);
        g.sessions.insert(session.id.clone(), session.clone());
        self.hub.publish("session", &session);
        drop(g);
        self.persist().await;
        Ok(session)
    }

    pub async fn resume(self: &Arc<Self>, id: &str) -> Result<Session, CreateError> {
        let _lifecycle = self.lifecycle.lock().await;
        let mut session = self
            .get(id)
            .await
            .ok_or_else(|| CreateError::not_found("No session with that id."))?;
        if session.live {
            return Err(CreateError::conflict("That session is already running."));
        }
        if session.archived {
            return Err(CreateError::conflict(
                "Restore that session before resuming it.",
            ));
        }
        mark_launching(&mut session, now_ms());
        self.start_process(&mut session).await?;
        self.persist().await;
        Ok(session)
    }

    pub async fn stop(&self, id: &str) -> Result<Session, CreateError> {
        let _lifecycle = self.lifecycle.lock().await;
        self.stop_process(id).await
    }

    async fn stop_process(&self, id: &str) -> Result<Session, CreateError> {
        let term = {
            let mut g = self.inner.lock().await;
            g.terminals.remove(id)
        };
        if let Some(term) = term {
            term.stop().await;
        }
        let mut g = self.inner.lock().await;
        let session = g
            .sessions
            .get_mut(id)
            .ok_or_else(|| CreateError::not_found("No session with that id."))?;
        let changed = session.live;
        if changed {
            mark_ended(session, None, now_ms());
            if let Some(g) = session.goal.as_mut() {
                syncaidync_core::goal::pause_for_end(g);
            }
        }
        let session = session.clone();
        g.signal_tokens.remove(id);
        g.activity.remove(id);
        if changed {
            self.hub.publish("session", &session);
        }
        drop(g);
        if changed {
            self.persist().await;
        }
        Ok(session)
    }

    pub async fn remove(&self, id: &str, remove_workspace: bool) -> Result<(), CreateError> {
        let _lifecycle = self.lifecycle.lock().await;
        let session = self.stop_process(id).await?;
        if remove_workspace {
            crate::workspaces::remove_owned(&self.home, &session.workspace)
                .await
                .map_err(CreateError::from_workspace)?;
        }
        self.inner.lock().await.sessions.remove(id);
        crate::goals::remove_record(&self.home, id);
        self.hub
            .publish("session_gone", &serde_json::json!({ "id": id }));
        self.persist().await;
        Ok(())
    }

    pub async fn removal_info(
        &self,
        id: &str,
    ) -> Result<crate::workspaces::RemovalInfo, CreateError> {
        let session = self
            .get(id)
            .await
            .ok_or_else(|| CreateError::not_found("No session with that id."))?;
        Ok(crate::workspaces::removal_info(
            &session.workspace.path,
            session.workspace.owned,
            session.workspace.base_commit.as_deref(),
        )
        .await)
    }

    pub async fn set_archived(&self, id: &str, archived: bool) -> Result<Session, CreateError> {
        let _lifecycle = self.lifecycle.lock().await;
        let mut g = self.inner.lock().await;
        let session = g
            .sessions
            .get_mut(id)
            .ok_or_else(|| CreateError::not_found("No session with that id."))?;
        if archived && (session.live || session.state != State::Ended) {
            return Err(CreateError::conflict(
                "Only ended sessions can be archived.",
            ));
        }
        if session.archived == archived {
            return Ok(session.clone());
        }
        session.archived = archived;
        session.updated_at = now_ms();
        let out = session.clone();
        self.hub.publish("session", &out);
        drop(g);
        self.persist().await;
        Ok(out)
    }

    pub async fn snooze(&self, id: &str, snoozed: bool) -> Result<Session, CreateError> {
        let mut g = self.inner.lock().await;
        let session = g
            .sessions
            .get_mut(id)
            .ok_or_else(|| CreateError::not_found("No session with that id."))?;
        session.snoozed = snoozed;
        session.updated_at = now_ms();
        let out = session.clone();
        drop(g);
        self.publish_session(&out).await;
        self.persist().await;
        Ok(out)
    }

    pub async fn rename(&self, id: &str, title: &str) -> Result<Session, CreateError> {
        let mut g = self.inner.lock().await;
        let session = g
            .sessions
            .get_mut(id)
            .ok_or_else(|| CreateError::not_found("No session with that id."))?;
        set_title(session, title, now_ms()).map_err(CreateError::bad_request)?;
        let out = session.clone();
        drop(g);
        self.publish_session(&out).await;
        self.persist().await;
        Ok(out)
    }

    pub async fn send(&self, id: &str, text: &str, submit: bool) -> Result<(), CreateError> {
        let term = self
            .terminal(id)
            .await
            .ok_or_else(|| CreateError::conflict("That session is not live."))?;
        send_text(&term, text, submit)
            .await
            .map_err(CreateError::bad_request)?;
        Ok(())
    }

    pub async fn apply_signal(&self, id: &str, signal: Signal) -> Result<(), CreateError> {
        let mut g = self.inner.lock().await;
        let session = g
            .sessions
            .get_mut(id)
            .ok_or_else(|| CreateError::not_found("No session with that id."))?;
        let (prev, prev_since) = (session.state, session.state_since);
        let now = now_ms();
        let changed = apply(session, &signal, now);
        let turn_ended = goal_turn_ended(session, prev, prev_since, now);
        let out = session.clone();
        drop(g);
        if changed {
            self.publish_session(&out).await;
            self.persist().await;
        }
        if turn_ended {
            self.spawn_after_turn(id);
        }
        Ok(())
    }

    pub async fn apply_agent_payload(
        &self,
        id: &str,
        agent: AgentId,
        payload: &serde_json::Value,
    ) -> Result<(), CreateError> {
        let signals = match agent {
            AgentId::Claude => ClaudeAdapter.signals(payload),
            AgentId::Shell => {
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
                ShellAdapter::new(shell).signals(payload)
            }
            other => adapter_for(other)
                .map(|a| a.signals(payload))
                .unwrap_or_default(),
        };

        let mut g = self.inner.lock().await;
        let session = g
            .sessions
            .get_mut(id)
            .ok_or_else(|| CreateError::not_found("No session with that id."))?;

        // Refresh Claude ids from any hook payload.
        if agent == AgentId::Claude {
            let (sid, transcript) = syncaidync_core::claude_ids(payload);
            if let Some(s) = sid {
                session.agent_session = Some(s);
            }
            if let Some(t) = transcript {
                session.transcript = Some(t);
            }
        }

        let mut changed = false;
        let now = now_ms();
        let mut turn_ended = false;
        for signal in &signals {
            let (prev, prev_since) = (session.state, session.state_since);
            if apply(session, signal, now) {
                changed = true;
            }
            turn_ended |= goal_turn_ended(session, prev, prev_since, now);
        }
        let out = session.clone();
        drop(g);
        if changed {
            self.publish_session(&out).await;
            self.persist().await;
        }
        if turn_ended {
            self.spawn_after_turn(id);
        }
        Ok(())
    }

    pub async fn set_usage(&self, id: &str, usage: Usage) -> Result<(), CreateError> {
        let mut g = self.inner.lock().await;
        let session = g
            .sessions
            .get_mut(id)
            .ok_or_else(|| CreateError::not_found("No session with that id."))?;
        // Cost/context are replaced when present (Claude sends running totals). Tokens from
        // Codex/Pi arrive as per-request deltas and are summed for the session.
        session.usage = Some(match session.usage.take() {
            None => usage,
            Some(old) => Usage {
                cost_usd: usage.cost_usd.or(old.cost_usd),
                context_pct: usage.context_pct.or(old.context_pct),
                tokens: match (old.tokens, usage.tokens) {
                    (Some(prev), Some(delta)) => Some(prev.saturating_add(delta)),
                    (None, Some(t)) | (Some(t), None) => Some(t),
                    (None, None) => None,
                },
            },
        });
        session.updated_at = now_ms();
        let goal_working = session
            .goal
            .as_ref()
            .is_some_and(|g| g.status == syncaidync_core::GoalStatus::Working);
        let out = session.clone();
        drop(g);
        self.publish_session(&out).await;
        // Coalescing usage events is fine to persist less often; still persist.
        self.persist().await;
        if goal_working {
            if let Some(reg) = self.me.upgrade() {
                crate::goals::enforce_limits(reg, id.to_string()).await;
            }
        }
        Ok(())
    }

    async fn clear_startup_block(&self, id: &str) {
        let out = {
            let mut g = self.inner.lock().await;
            let Some(session) = g.sessions.get_mut(id) else {
                return;
            };
            if session.state == State::Blocked
                && session
                    .blocked
                    .as_ref()
                    .is_some_and(|b| b.kind == BlockedKind::Startup)
            {
                mark_launching(session, now_ms());
                Some(session.clone())
            } else {
                None
            }
        };
        if let Some(out) = out {
            self.publish_session(&out).await;
            self.persist().await;
        }
    }

    async fn apply_activity(&self, id: &str, event: ActivityEvent) {
        let signals = {
            let mut g = self.inner.lock().await;
            let state = match g.sessions.get(id) {
                Some(s) => s.state,
                None => return,
            };
            let watcher = match g.activity.get_mut(id) {
                Some(w) => w,
                None => return,
            };
            watcher.handle(state, event)
        };
        let signals = self.shell_questions(id, signals).await;
        for signal in signals {
            let _ = self.apply_signal(id, signal).await;
        }
    }

    /// A shell has no hooks, so a finished turn whose last line asks for an answer (a
    /// password, a yes/no) becomes a question instead of going ready.
    async fn shell_questions(&self, id: &str, signals: Vec<Signal>) -> Vec<Signal> {
        if !signals.contains(&Signal::TurnEnded) {
            return signals;
        }
        let term = {
            let g = self.inner.lock().await;
            if g.sessions.get(id).map(|s| s.agent) != Some(AgentId::Shell) {
                return signals;
            }
            g.terminals.get(id).cloned()
        };
        let Some(term) = term else { return signals };
        let bytes = term.snapshot().await;
        let start = bytes.len().saturating_sub(4096);
        let Some(message) = shell_question(&String::from_utf8_lossy(&bytes[start..])) else {
            return signals;
        };
        signals
            .into_iter()
            .map(|s| match s {
                Signal::TurnEnded => Signal::QuestionAsked {
                    message: message.clone(),
                },
                other => other,
            })
            .collect()
    }

    async fn check_startup_markers(&self, id: &str) {
        let (state, markers, snap) = {
            let g = self.inner.lock().await;
            let Some(session) = g.sessions.get(id) else {
                return;
            };
            if session.state != State::Launching {
                return;
            }
            let markers = match session.agent {
                AgentId::Shell => {
                    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
                    ShellAdapter::new(shell).startup_markers()
                }
                other => adapter_for(other)
                    .map(|a| a.startup_markers())
                    .unwrap_or(&[]),
            };
            let term = g.terminals.get(id).cloned();
            (session.state, markers, term)
        };
        let _ = state;
        let Some(term) = snap else { return };
        let bytes = term.snapshot().await;
        let start = bytes.len().saturating_sub(4096);
        let text = String::from_utf8_lossy(&bytes[start..]);
        let normalized = normalize_tail(&text);
        if let Some(m) = find_marker(&normalized, markers) {
            let mut g = self.inner.lock().await;
            let Some(session) = g.sessions.get_mut(id) else {
                return;
            };
            if session.state != State::Launching {
                return;
            }
            mark_startup_blocked(session, m.message, now_ms());
            let out = session.clone();
            drop(g);
            self.publish_session(&out).await;
            self.persist().await;
        }
    }

    async fn on_exit(&self, id: String, token: &str, code: Option<i32>) {
        {
            let mut g = self.inner.lock().await;
            if g.signal_tokens.get(&id).map(String::as_str) != Some(token) {
                return;
            }
            g.terminals.remove(&id);
            g.signal_tokens.remove(&id);
            g.activity.remove(&id);
            let Some(session) = g.sessions.get_mut(&id) else {
                return;
            };
            if !session.live {
                return;
            }
            mark_ended(session, code, now_ms());
            if let Some(g) = session.goal.as_mut() {
                syncaidync_core::goal::pause_for_end(g);
            }
            self.hub.publish("session", session);
        }
        self.persist().await;
    }

    pub async fn shutdown(&self) {
        let terms: Vec<_> = {
            let mut g = self.inner.lock().await;
            g.terminals.drain().map(|(_, t)| t).collect()
        };
        for t in &terms {
            t.stop().await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        self.persist().await;
        self.saver.flush().await;
    }

    async fn start_process(self: &Arc<Self>, session: &mut Session) -> Result<(), CreateError> {
        let token = random_hex(32);
        let run_dir = self.home.join("run").join(&session.id);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::create_dir_all(&run_dir)
                .map_err(|e| CreateError::internal(format!("run dir: {e}")))?;
            let _ = std::fs::set_permissions(&run_dir, std::fs::Permissions::from_mode(0o700));
            if let Some(parent) = run_dir.parent() {
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }
        #[cfg(not(unix))]
        {
            std::fs::create_dir_all(&run_dir)
                .map_err(|e| CreateError::internal(format!("run dir: {e}")))?;
        }

        let hook_bin = std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "syncaidync".into());
        let run_dir_str = run_dir.to_string_lossy().into_owned();
        // Resume when the session already carries a CLI conversation id.
        let had_agent_session = session.agent_session.is_some();
        // A goal session's first prompt is the brief: the person's prompt plus the goal.
        // A shell would run the brief as commands, so it only gets the person's prompt.
        let launch_prompt: Option<String> = if session.agent == AgentId::Shell {
            session.prompt.clone()
        } else {
            crate::goals::brief_for(session).or_else(|| session.prompt.clone())
        };
        let req = syncaidync_core::LaunchRequest {
            session_id: &session.id,
            agent_session: session.agent_session.as_deref(),
            resume: had_agent_session,
            model: session.model.as_deref(),
            autonomy: session.autonomy,
            prompt: if had_agent_session {
                None
            } else {
                launch_prompt.as_deref()
            },
            hook_bin: &hook_bin,
            run_dir: &run_dir_str,
        };

        let (plan, observes, had_cli_prompt) = match session.agent {
            AgentId::Shell => {
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
                let adapter = ShellAdapter::new(shell);
                let plan = adapter.launch(&req);
                (plan, adapter.observes(), false)
            }
            other => {
                let adapter = adapter_for(other).ok_or_else(|| {
                    CreateError::unprocessable(format!("Unknown agent: {}", other.as_str()))
                })?;
                let plan = adapter.launch(&req);
                let had_cli =
                    !had_agent_session && launch_prompt.as_ref().is_some_and(|p| !p.is_empty());
                (plan, adapter.observes(), had_cli)
            }
        };

        if let Some(id) = plan.agent_session {
            session.agent_session = Some(id);
        }
        for (path, contents) in &plan.files {
            if let Some(parent) = Path::new(path).parent() {
                let _ = std::fs::create_dir_all(parent);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ =
                        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
                }
            }
            std::fs::write(path, contents)
                .map_err(|e| CreateError::internal(format!("write {path}: {e}")))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            }
        }

        let (plan_bin, args) = match plan.args.split_first() {
            Some((b, rest)) => (b.clone(), rest.to_vec()),
            None => return Err(CreateError::internal("launch plan has no args")),
        };
        let bin = if session.agent == AgentId::Shell {
            plan_bin
        } else {
            agents_discover::resolve_bin_for_launch(session.agent, &plan_bin)
                .map_err(CreateError::unprocessable)?
        };

        let mut env = scrubbed_env();
        env.push(("SYNCAIDYNC".into(), "1".into()));
        env.push((
            "SYNCAIDYNC_URL".into(),
            format!("http://127.0.0.1:{}", self.port),
        ));
        env.push(("SYNCAIDYNC_SESSION".into(), session.id.clone()));
        env.push(("SYNCAIDYNC_SIGNAL_TOKEN".into(), token.clone()));
        env.push(("TERM".into(), "xterm-256color".into()));
        env.push(("COLORTERM".into(), "truecolor".into()));
        // BSD/macOS `ls` and friends only colour with CLICOLOR. Respect an existing
        // CLICOLOR or a NO_COLOR opt-out from the user's environment.
        if !env.iter().any(|(k, _)| k == "CLICOLOR" || k == "NO_COLOR") {
            env.push(("CLICOLOR".into(), "1".into()));
        }
        env.extend(plan.env);

        let log_path = self.home.join("logs").join(format!("{}.log", session.id));
        let reg = Arc::clone(self);
        let exit_id = session.id.clone();
        let exit_token = token.clone();
        let on_exit = Arc::new(move |code: Option<i32>| {
            let reg = reg.clone();
            let id = exit_id.clone();
            let token = exit_token.clone();
            tokio::spawn(async move {
                reg.on_exit(id, &token, code).await;
            });
        });

        let watch_activity = observes == Observes::Activity;
        let out_id = session.id.clone();
        let reg_out = Arc::clone(self);
        let on_output: Option<OutputHook> = Some(Arc::new(move |_chunk: bytes::Bytes| {
            let reg = reg_out.clone();
            let id = out_id.clone();
            tokio::spawn(async move {
                if watch_activity {
                    reg.apply_activity(&id, ActivityEvent::Output { at: now_ms() })
                        .await;
                }
                reg.check_startup_markers(&id).await;
            });
        }));

        let in_id = session.id.clone();
        let reg_in = Arc::clone(self);
        let on_input: Option<InputHook> = Some(Arc::new(move |data: &[u8]| {
            let has_return = data.contains(&b'\r') || data.contains(&b'\n');
            let reg = reg_in.clone();
            let id = in_id.clone();
            tokio::spawn(async move {
                if has_return {
                    reg.clear_startup_block(&id).await;
                }
                if watch_activity {
                    reg.apply_activity(
                        &id,
                        ActivityEvent::Input {
                            has_return,
                            at: now_ms(),
                        },
                    )
                    .await;
                }
            });
        }));

        // Register the session before callbacks can observe a partially started process.
        let mut g = self.inner.lock().await;
        let term = Terminal::spawn(SpawnOpts {
            bin,
            args,
            env,
            cwd: PathBuf::from(&session.workspace.path),
            rows: 24,
            cols: 80,
            log_path,
            on_output,
            on_input,
            on_exit: Some(on_exit),
        })
        .await
        .map_err(CreateError::unprocessable)?;

        let term = Arc::new(term);
        g.sessions.insert(session.id.clone(), session.clone());
        g.terminals.insert(session.id.clone(), term.clone());
        g.signal_tokens.insert(session.id.clone(), token);
        g.activity
            .insert(session.id.clone(), ActivityWatcher::new(had_cli_prompt));
        self.hub.publish("session", session);
        drop(g);

        // Activity tick loop
        if watch_activity {
            let reg = Arc::clone(self);
            let tick_id = session.id.clone();
            let tick_term = Arc::downgrade(&term);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
                loop {
                    interval.tick().await;
                    let current = reg.terminal(&tick_id).await;
                    if !current.is_some_and(|term| Arc::downgrade(&term).ptr_eq(&tick_term)) {
                        break;
                    }
                    reg.apply_activity(&tick_id, ActivityEvent::Tick { at: now_ms() })
                        .await;
                }
            });
        }

        // Shell: type the first prompt after attach. Other agents take it on argv.
        if session.agent == AgentId::Shell {
            if let Some(prompt) = launch_prompt.clone() {
                let term = term.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    let _ = send_text(&term, &prompt, true).await;
                });
            }
        }

        Ok(())
    }

    async fn build_workspace(
        &self,
        session_id: &str,
        spec: CreateWorkspace,
    ) -> Result<Workspace, CreateError> {
        match spec {
            CreateWorkspace::Worktree { repo, base, branch } => {
                crate::workspaces::create_worktree(&self.home, &repo, &base, &branch)
                    .await
                    .map_err(CreateError::from_workspace)
            }
            CreateWorkspace::Folder { path } => crate::workspaces::create_folder(&path)
                .await
                .map_err(CreateError::from_workspace),
            CreateWorkspace::Scratch {} => {
                crate::workspaces::create_scratch(&self.home, session_id)
                    .map_err(CreateError::from_workspace)
            }
        }
    }

    async fn publish_session(&self, session: &Session) {
        self.hub.publish("session", session);
    }

    async fn persist(&self) {
        let sessions = {
            let g = self.inner.lock().await;
            g.sessions.values().cloned().collect()
        };
        self.saver
            .queue(SessionsFile {
                version: 1,
                sessions,
            })
            .await;
    }
}

/// Bank working time and report whether this state change ends a turn the goal must check.
fn goal_turn_ended(session: &mut Session, prev: State, prev_since: u64, now: u64) -> bool {
    let next = session.state;
    match session.goal.as_mut() {
        Some(g) => syncaidync_core::goal::note_state_change(g, prev, prev_since, next, now),
        None => false,
    }
}

fn parse_agent(s: &str) -> Result<AgentId, CreateError> {
    match s {
        "claude" => Ok(AgentId::Claude),
        "codex" => Ok(AgentId::Codex),
        "pi" => Ok(AgentId::Pi),
        "cursor" => Ok(AgentId::Cursor),
        "shell" => Ok(AgentId::Shell),
        _ => Err(CreateError::bad_request(format!("Unknown agent: {s}"))),
    }
}

fn scrubbed_env() -> Vec<(String, String)> {
    let drop = [
        "CLAUDECODE",
        "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_CODE_SSE_PORT",
        "CURSOR_AGENT",
        "CODEX_SANDBOX",
        "CODEX_SANDBOX_NETWORK_DISABLED",
        "SYNCAIDYNC_KEY",
        "SYNCAIDYNC_SESSION",
        "SYNCAIDYNC_SIGNAL_TOKEN",
    ];
    std::env::vars()
        .filter(|(k, _)| !drop.contains(&k.as_str()))
        .collect()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug)]
pub struct CreateError {
    pub status: hyper::StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl CreateError {
    pub fn bad_request(m: impl Into<String>) -> Self {
        Self {
            status: hyper::StatusCode::BAD_REQUEST,
            code: "bad_request",
            message: m.into(),
        }
    }
    pub fn not_found(m: impl Into<String>) -> Self {
        Self {
            status: hyper::StatusCode::NOT_FOUND,
            code: "not_found",
            message: m.into(),
        }
    }
    pub fn conflict(m: impl Into<String>) -> Self {
        Self {
            status: hyper::StatusCode::CONFLICT,
            code: "conflict",
            message: m.into(),
        }
    }
    pub fn unprocessable(m: impl Into<String>) -> Self {
        Self {
            status: hyper::StatusCode::UNPROCESSABLE_ENTITY,
            code: "unprocessable",
            message: m.into(),
        }
    }
    pub fn internal(m: impl Into<String>) -> Self {
        Self {
            status: hyper::StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal",
            message: m.into(),
        }
    }

    pub fn into_api(self) -> crate::http::error::ApiError {
        crate::http::error::ApiError::new(self.status, self.code, self.message)
    }

    fn from_workspace(e: crate::workspaces::WorkspaceError) -> Self {
        match e {
            crate::workspaces::WorkspaceError::BadRequest(m) => Self::bad_request(m),
            crate::workspaces::WorkspaceError::Conflict(m) => Self::conflict(m),
            crate::workspaces::WorkspaceError::Unprocessable(m) => Self::unprocessable(m),
            crate::workspaces::WorkspaceError::Internal(m) => Self::internal(m),
        }
    }
}
