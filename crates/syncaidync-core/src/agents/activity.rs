//! Infer session signals from terminal input and output for hookless agents.

use crate::model::State;
use crate::signal::Signal;

const QUIET_WORKING_MS: u64 = 5_000;
const QUIET_LAUNCH_MS: u64 = 2_000;
const READY_FLOW_MS: u64 = 1_500;

/// Input to the activity watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityEvent {
    Output { at: u64 },
    Input { has_return: bool, at: u64 },
    Tick { at: u64 },
}

/// Approximate state machine for agents that observe by activity.
#[derive(Debug, Clone)]
pub struct ActivityWatcher {
    /// True when the agent was started with a first prompt on the command line.
    pub had_cli_prompt: bool,
    first_output_at: Option<u64>,
    last_output_at: Option<u64>,
    /// While `ready`, when continuous output (with no intervening input) began.
    ready_flow_since: Option<u64>,
    /// After emitting SessionStarted from quiet launch, do not emit it again.
    launched: bool,
}

impl ActivityWatcher {
    pub fn new(had_cli_prompt: bool) -> Self {
        Self {
            had_cli_prompt,
            first_output_at: None,
            last_output_at: None,
            ready_flow_since: None,
            launched: false,
        }
    }

    /// Handle one event given the session's current state. Returns signals to apply.
    pub fn handle(&mut self, session_state: State, event: ActivityEvent) -> Vec<Signal> {
        match event {
            ActivityEvent::Output { at } => self.on_output(session_state, at),
            ActivityEvent::Input { has_return, at } => self.on_input(session_state, has_return, at),
            ActivityEvent::Tick { at } => self.on_tick(session_state, at),
        }
    }

    fn on_output(&mut self, state: State, at: u64) -> Vec<Signal> {
        self.last_output_at = Some(at);
        match state {
            State::Working => Vec::new(),
            State::Launching => {
                if self.first_output_at.is_none() {
                    self.first_output_at = Some(at);
                    if self.had_cli_prompt {
                        return vec![Signal::PromptSubmitted];
                    }
                }
                Vec::new()
            }
            State::Ready => {
                if self.ready_flow_since.is_none() {
                    self.ready_flow_since = Some(at);
                }
                if let Some(since) = self.ready_flow_since {
                    if at.saturating_sub(since) >= READY_FLOW_MS {
                        self.ready_flow_since = None;
                        return vec![Signal::PromptSubmitted];
                    }
                }
                Vec::new()
            }
            State::Blocked | State::Ended => Vec::new(),
        }
    }

    fn on_input(&mut self, state: State, has_return: bool, _at: u64) -> Vec<Signal> {
        // Typing resets the "flowing output while ready" timer so an echo does not flip state.
        self.ready_flow_since = None;
        // Blocked here is a shell question (startup blocks are cleared before this runs):
        // answering it starts a new turn.
        if has_return && matches!(state, State::Ready | State::Launching | State::Blocked) {
            return vec![Signal::PromptSubmitted];
        }
        Vec::new()
    }

    fn on_tick(&mut self, state: State, at: u64) -> Vec<Signal> {
        match state {
            State::Working => {
                if let Some(last) = self.last_output_at {
                    if at.saturating_sub(last) >= QUIET_WORKING_MS {
                        self.last_output_at = None;
                        return vec![Signal::TurnEnded];
                    }
                }
                Vec::new()
            }
            State::Launching if !self.had_cli_prompt && !self.launched => {
                if let (Some(_first), Some(last)) = (self.first_output_at, self.last_output_at) {
                    if at.saturating_sub(last) >= QUIET_LAUNCH_MS {
                        self.launched = true;
                        return vec![Signal::SessionStarted {
                            agent_session: None,
                            transcript: None,
                        }];
                    }
                }
                Vec::new()
            }
            State::Ready => {
                if let Some(since) = self.ready_flow_since {
                    if at.saturating_sub(since) >= READY_FLOW_MS {
                        // Still flowing if last output is recent; otherwise reset.
                        if self
                            .last_output_at
                            .is_some_and(|last| at.saturating_sub(last) < READY_FLOW_MS)
                        {
                            self.ready_flow_since = None;
                            return vec![Signal::PromptSubmitted];
                        }
                        self.ready_flow_since = None;
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answering_while_blocked_submits() {
        let mut w = ActivityWatcher::new(false);
        let sigs = w.handle(
            State::Blocked,
            ActivityEvent::Input {
                has_return: true,
                at: 10,
            },
        );
        assert_eq!(sigs, vec![Signal::PromptSubmitted]);
    }

    #[test]
    fn cli_prompt_first_output_submits() {
        let mut w = ActivityWatcher::new(true);
        let sigs = w.handle(State::Launching, ActivityEvent::Output { at: 10 });
        assert_eq!(sigs, vec![Signal::PromptSubmitted]);
    }

    #[test]
    fn no_prompt_quiet_after_output_starts() {
        let mut w = ActivityWatcher::new(false);
        assert!(w
            .handle(State::Launching, ActivityEvent::Output { at: 10 })
            .is_empty());
        assert!(w
            .handle(State::Launching, ActivityEvent::Tick { at: 1000 })
            .is_empty());
        let sigs = w.handle(State::Launching, ActivityEvent::Tick { at: 2010 });
        assert_eq!(
            sigs,
            vec![Signal::SessionStarted {
                agent_session: None,
                transcript: None,
            }]
        );
    }

    #[test]
    fn working_quiet_timer() {
        let mut w = ActivityWatcher::new(false);
        w.handle(State::Working, ActivityEvent::Output { at: 100 });
        assert!(w
            .handle(State::Working, ActivityEvent::Tick { at: 4_000 })
            .is_empty());
        let sigs = w.handle(State::Working, ActivityEvent::Tick { at: 5_100 });
        assert_eq!(sigs, vec![Signal::TurnEnded]);
    }

    #[test]
    fn typing_echo_does_not_flip_ready() {
        let mut w = ActivityWatcher::new(false);
        w.handle(State::Ready, ActivityEvent::Output { at: 100 });
        w.handle(
            State::Ready,
            ActivityEvent::Input {
                has_return: false,
                at: 200,
            },
        );
        // Output after typing starts a new flow window.
        w.handle(State::Ready, ActivityEvent::Output { at: 300 });
        assert!(w
            .handle(State::Ready, ActivityEvent::Tick { at: 1_000 })
            .is_empty());
    }

    #[test]
    fn long_output_while_ready_submits() {
        let mut w = ActivityWatcher::new(false);
        w.handle(State::Ready, ActivityEvent::Output { at: 0 });
        w.handle(State::Ready, ActivityEvent::Output { at: 1_000 });
        let sigs = w.handle(State::Ready, ActivityEvent::Output { at: 1_600 });
        assert_eq!(sigs, vec![Signal::PromptSubmitted]);
    }

    #[test]
    fn return_while_ready_submits() {
        let mut w = ActivityWatcher::new(false);
        let sigs = w.handle(
            State::Ready,
            ActivityEvent::Input {
                has_return: true,
                at: 1,
            },
        );
        assert_eq!(sigs, vec![Signal::PromptSubmitted]);
    }
}
