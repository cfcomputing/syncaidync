//! Development shell agent: runs a login shell with no hooks.

use crate::model::Usage;
use crate::signal::Signal;
use crate::startup::{strip_ansi, Marker};

use super::{Adapter, AgentId, LaunchPlan, LaunchRequest, Observes, OneShotPlan};

/// Runs the given shell binary with `-i`. The path is supplied by the caller.
#[derive(Debug, Clone)]
pub struct ShellAdapter {
    pub shell: String,
}

impl ShellAdapter {
    pub fn new(shell: impl Into<String>) -> Self {
        Self {
            shell: shell.into(),
        }
    }
}

impl Adapter for ShellAdapter {
    fn id(&self) -> AgentId {
        AgentId::Shell
    }

    fn label(&self) -> &'static str {
        "Shell"
    }

    fn bin_names(&self) -> &'static [&'static str] {
        &[]
    }

    fn observes(&self) -> Observes {
        Observes::Activity
    }

    fn models(&self) -> &'static [&'static str] {
        &[]
    }

    fn launch(&self, req: &LaunchRequest<'_>) -> LaunchPlan {
        // A first prompt is typed by the registry after attach, not passed as argv.
        let _ = req;
        LaunchPlan {
            args: vec![self.shell.clone(), "-i".into()],
            env: Vec::new(),
            files: Vec::new(),
            agent_session: None,
        }
    }

    fn signals(&self, _payload: &serde_json::Value) -> Vec<Signal> {
        Vec::new()
    }

    fn usage(&self, _payload: &serde_json::Value) -> Option<Usage> {
        None
    }

    fn startup_markers(&self) -> &'static [Marker] {
        // Distinctive phrase for the WO-05 integration test (shell has no real CLI prompts).
        &[Marker {
            needle: "syncaidync-test-startup-block",
            message: "Test startup block",
        }]
    }

    fn oneshot(&self, _model: Option<&str>, _prompt: &str) -> OneShotPlan {
        OneShotPlan {
            args: vec![self.shell.clone(), "-c".into(), "echo unsupported".into()],
            stdin: None,
        }
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

/// Longest question text kept for the blocked banner.
const QUESTION_MAX: usize = 160;

/// If the shell's output ends on a line that asks for an answer (a password, a yes/no, a
/// "Continue?"), return that line. `tail` is raw terminal output; only its last line counts.
pub fn shell_question(tail: &str) -> Option<String> {
    let text = strip_ansi(tail);
    let line = text
        .split('\n')
        .map(|l| l.rsplit('\r').next().unwrap_or(l).trim())
        .rfind(|l| !l.is_empty())?;
    let lower = line.to_lowercase();
    let asks = lower.ends_with('?')
        || ((lower.contains("password") || lower.contains("passphrase")) && lower.ends_with(':'))
        || ["[y/n]", "(y/n)", "[yes/no]", "(yes/no)", "[y/n/", "(y/n/"]
            .iter()
            .any(|p| lower.contains(p))
        || lower.contains("press enter")
        || lower.contains("press return")
        || lower.contains("press any key");
    if !asks {
        return None;
    }
    let mut out: String = line.chars().take(QUESTION_MAX).collect();
    if line.chars().count() > QUESTION_MAX {
        out.push('…');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn question_lines_are_found() {
        for (tail, want) in [
            ("$ sudo ls\r\nPassword:", "Password:"),
            ("[sudo] password for cf: ", "[sudo] password for cf:"),
            (
                "Enter passphrase for key '/k':",
                "Enter passphrase for key '/k':",
            ),
            ("Remove file? [y/N] ", "Remove file? [y/N]"),
            ("Proceed (y/n)", "Proceed (y/n)"),
            (
                "\x1b[1mOverwrite existing files?\x1b[0m",
                "Overwrite existing files?",
            ),
            ("Press ENTER to continue", "Press ENTER to continue"),
            (
                "old line\rDo you want to continue?",
                "Do you want to continue?",
            ),
        ] {
            assert_eq!(shell_question(tail).as_deref(), Some(want), "tail {tail:?}");
        }
    }

    #[test]
    fn prompts_and_output_are_not_questions() {
        for tail in [
            "cf@mac ~/repo % ",
            "build finished in 2.1s\r\n$ ",
            "Is this a question?\r\n❯ ",
            "Listening on http://127.0.0.1:3000",
            "",
            "PIN code set",
        ] {
            assert_eq!(shell_question(tail), None, "tail {tail:?}");
        }
    }

    #[test]
    fn long_questions_are_cut() {
        let q = format!("{}?", "a".repeat(400));
        let got = shell_question(&q).unwrap();
        assert_eq!(got.chars().count(), QUESTION_MAX + 1);
        assert!(got.ends_with('…'));
    }

    #[test]
    fn launch_is_interactive_shell() {
        let a = ShellAdapter::new("/bin/zsh");
        let plan = a.launch(&LaunchRequest {
            session_id: "s",
            agent_session: None,
            resume: false,
            model: None,
            autonomy: false,
            prompt: Some("hi"),
            hook_bin: "/bin/syncaidync",
            run_dir: "/tmp",
        });
        assert_eq!(plan.args, vec!["/bin/zsh", "-i"]);
        assert!(plan.files.is_empty());
        assert_eq!(a.observes(), Observes::Activity);
    }
}
