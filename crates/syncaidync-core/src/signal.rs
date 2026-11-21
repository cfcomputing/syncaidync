//! Signals from agent hooks and the session state reducer.

use crate::model::{push_tool, set_state, Blocked, BlockedKind, Session, State};

/// A normalized report from an agent hook or the activity watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    SessionStarted {
        agent_session: Option<String>,
        transcript: Option<String>,
    },
    PromptSubmitted,
    ToolStarting {
        name: String,
        detail: String,
    },
    ToolFinished {
        name: String,
    },
    ApprovalNeeded {
        message: String,
    },
    QuestionAsked {
        message: String,
    },
    TurnEnded,
    SessionEnded,
    Other,
}

/// Apply a signal to a session. Returns true if anything changed.
pub fn apply(session: &mut Session, signal: &Signal, now_ms: u64) -> bool {
    if session.state == State::Ended {
        return apply_ended(session, signal, now_ms);
    }

    match signal {
        Signal::SessionStarted {
            agent_session,
            transcript,
        } => {
            let mut changed = false;
            if let Some(id) = agent_session {
                if session.agent_session.as_deref() != Some(id) {
                    session.agent_session = Some(id.clone());
                    changed = true;
                }
            }
            if let Some(path) = transcript {
                if session.transcript.as_deref() != Some(path) {
                    session.transcript = Some(path.clone());
                    changed = true;
                }
            }
            if session.state == State::Launching {
                set_state(session, State::Ready, None, now_ms);
                changed = true;
            }
            if changed {
                session.updated_at = now_ms;
            }
            changed
        }
        Signal::PromptSubmitted => go_working(session, now_ms),
        Signal::ToolStarting { name, detail } => {
            let changed = go_working(session, now_ms);
            push_tool(session, name.clone(), detail.clone(), now_ms);
            session.updated_at = now_ms;
            // push_tool always changes tools
            let _ = changed;
            true
        }
        Signal::ToolFinished { .. } => {
            if matches!(session.state, State::Blocked | State::Launching) {
                go_working(session, now_ms)
            } else {
                false
            }
        }
        Signal::ApprovalNeeded { message } => {
            set_blocked(session, BlockedKind::Approval, message, now_ms)
        }
        Signal::QuestionAsked { message } => {
            set_blocked(session, BlockedKind::Question, message, now_ms)
        }
        Signal::TurnEnded => {
            if session.state == State::Ready && session.blocked.is_none() {
                return false;
            }
            set_state(session, State::Ready, None, now_ms);
            session.updated_at = now_ms;
            true
        }
        Signal::SessionEnded => {
            if matches!(session.state, State::Working | State::Blocked) {
                set_state(session, State::Ready, None, now_ms);
                session.updated_at = now_ms;
                true
            } else {
                false
            }
        }
        Signal::Other => false,
    }
}

fn apply_ended(session: &mut Session, signal: &Signal, now_ms: u64) -> bool {
    match signal {
        Signal::SessionStarted {
            agent_session,
            transcript,
        } => {
            let mut changed = false;
            if let Some(id) = agent_session {
                if session.agent_session.as_deref() != Some(id) {
                    session.agent_session = Some(id.clone());
                    changed = true;
                }
            }
            if let Some(path) = transcript {
                if session.transcript.as_deref() != Some(path) {
                    session.transcript = Some(path.clone());
                    changed = true;
                }
            }
            if changed {
                session.updated_at = now_ms;
            }
            changed
        }
        _ => false,
    }
}

fn go_working(session: &mut Session, now_ms: u64) -> bool {
    let was = session.state;
    let had_blocked = session.blocked.is_some();
    let was_snoozed = session.snoozed;
    set_state(session, State::Working, None, now_ms);
    session.snoozed = false;
    session.updated_at = now_ms;
    was != State::Working || had_blocked || was_snoozed
}

fn set_blocked(session: &mut Session, kind: BlockedKind, message: &str, now_ms: u64) -> bool {
    let same = session.state == State::Blocked
        && session
            .blocked
            .as_ref()
            .is_some_and(|b| b.kind == kind && b.message == message);
    if same {
        return false;
    }
    set_state(
        session,
        State::Blocked,
        Some(Blocked {
            kind,
            message: message.to_string(),
        }),
        now_ms,
    );
    session.updated_at = now_ms;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentId;
    use crate::model::{mark_ended, NewSession, Workspace, WorkspaceKind};

    fn session(state: State) -> Session {
        let mut s = Session::new(NewSession {
            id: "s".into(),
            agent: AgentId::Claude,
            model: None,
            autonomy: false,
            prompt: None,
            title: Some("t".into()),
            workspace: Workspace {
                kind: WorkspaceKind::Scratch,
                path: "/tmp".into(),
                repo: None,
                branch: None,
                base: None,
                base_commit: None,
                owned: true,
            },
            pull: None,
            now_ms: 1000,
        });
        match state {
            State::Launching => {}
            State::Working => {
                apply(&mut s, &Signal::PromptSubmitted, 1001);
            }
            State::Ready => {
                apply(
                    &mut s,
                    &Signal::SessionStarted {
                        agent_session: None,
                        transcript: None,
                    },
                    1001,
                );
            }
            State::Blocked => {
                apply(
                    &mut s,
                    &Signal::ApprovalNeeded {
                        message: "x".into(),
                    },
                    1001,
                );
            }
            State::Ended => {
                mark_ended(&mut s, Some(0), 1001);
            }
        }
        s
    }

    fn all_signals() -> Vec<Signal> {
        vec![
            Signal::SessionStarted {
                agent_session: Some("a".into()),
                transcript: Some("/t".into()),
            },
            Signal::PromptSubmitted,
            Signal::ToolStarting {
                name: "Bash".into(),
                detail: "ls".into(),
            },
            Signal::ToolFinished {
                name: "Bash".into(),
            },
            Signal::ApprovalNeeded {
                message: "approve".into(),
            },
            Signal::QuestionAsked {
                message: "q".into(),
            },
            Signal::TurnEnded,
            Signal::SessionEnded,
            Signal::Other,
        ]
    }

    #[test]
    fn table_every_state_every_signal() {
        for state in [
            State::Launching,
            State::Working,
            State::Blocked,
            State::Ready,
            State::Ended,
        ] {
            for signal in all_signals() {
                let mut s = session(state);
                let before = s.state;
                apply(&mut s, &signal, 2000);
                if before == State::Ended {
                    assert_eq!(s.state, State::Ended, "{state:?} + {signal:?}");
                }
            }
        }
    }

    #[test]
    fn working_clears_snooze() {
        let mut s = session(State::Ready);
        s.snoozed = true;
        assert!(apply(&mut s, &Signal::PromptSubmitted, 50));
        assert!(!s.snoozed);
        assert_eq!(s.state, State::Working);
    }

    #[test]
    fn state_since_only_on_state_or_kind_change() {
        let mut s = session(State::Working);
        let since = s.state_since;
        apply(
            &mut s,
            &Signal::ToolStarting {
                name: "Bash".into(),
                detail: "a".into(),
            },
            3000,
        );
        assert_eq!(s.state_since, since);
        apply(
            &mut s,
            &Signal::ApprovalNeeded {
                message: "one".into(),
            },
            4000,
        );
        assert_eq!(s.state_since, 4000);
        apply(
            &mut s,
            &Signal::ApprovalNeeded {
                message: "two".into(),
            },
            5000,
        );
        assert_eq!(s.state_since, 4000); // kind unchanged
        apply(
            &mut s,
            &Signal::QuestionAsked {
                message: "q".into(),
            },
            6000,
        );
        assert_eq!(s.state_since, 6000);
    }

    #[test]
    fn ended_updates_ids_not_state() {
        let mut s = session(State::Ended);
        assert!(apply(
            &mut s,
            &Signal::SessionStarted {
                agent_session: Some("new".into()),
                transcript: Some("/p".into()),
            },
            9,
        ));
        assert_eq!(s.agent_session.as_deref(), Some("new"));
        assert_eq!(s.state, State::Ended);
        assert!(!apply(&mut s, &Signal::PromptSubmitted, 10));
        assert_eq!(s.state, State::Ended);
    }

    #[test]
    fn session_ended_from_working_goes_ready() {
        let mut s = session(State::Working);
        assert!(apply(&mut s, &Signal::SessionEnded, 7));
        assert_eq!(s.state, State::Ready);
    }
}
