//! Goals: run checks in a session's workspace, guard the paths they rely on, and drive the
//! session round by round. The decisions themselves live in `syncaidync_core::goal`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use syncaidync_core::goal::{
    self, build_review_prompt, compose_brief, decide, decide_review, interrupt_bytes, over_limit,
    parse_review, working_ms_now, CheckStage, Limits, REVIEW_BUDGET,
};
use syncaidync_core::{strip_ansi, AgentId, CheckOutcome, Decision, Goal, GoalStatus, Session};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::{Mutex, Semaphore};

use crate::changes::{self, ChangeSource};
use crate::registry::{CreateError, Registry};

const OUTPUT_KEEP: usize = 64 * 1024;
const OUTPUT_STORE: usize = 16 * 1024;
const ROUNDS_KEPT: usize = 10;
const MAX_GUARD_FILES: usize = 2000;
const MAX_GUARD_BYTES: u64 = 50 * 1024 * 1024;

fn semaphore() -> &'static Semaphore {
    static SEM: OnceLock<Semaphore> = OnceLock::new();
    SEM.get_or_init(|| Semaphore::new(2))
}

fn file_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ---------------------------------------------------------------------------------------------
// The record file: rounds with output, and guard hashes (never sent to the browser)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundRecord {
    pub round: u32,
    pub at: u64,
    pub results: Vec<CheckOutcome>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RecordFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    rounds: Vec<RoundRecord>,
    #[serde(default)]
    hashes: BTreeMap<String, BTreeMap<String, String>>,
}

fn record_path(home: &Path, id: &str) -> PathBuf {
    home.join("goals").join(format!("{id}.json"))
}

fn load_record(home: &Path, id: &str) -> RecordFile {
    crate::store::load::<RecordFile>(&record_path(home, id))
        .ok()
        .flatten()
        .unwrap_or_default()
}

fn save_record(home: &Path, id: &str, rec: &RecordFile) {
    let mut rec = rec.clone();
    rec.version = 1;
    if let Ok(bytes) = serde_json::to_vec(&rec) {
        if let Err(e) = crate::store::atomic_write(&record_path(home, id), &bytes) {
            eprintln!("syncaidync: could not save goal record: {e}");
        }
    }
}

pub fn remove_record(home: &Path, id: &str) {
    let _ = std::fs::remove_file(record_path(home, id));
}

/// The rounds stored for a session, for `GET /sessions/{id}/goal`.
pub async fn rounds(home: &Path, id: &str) -> Vec<RoundRecord> {
    let _g = file_lock().lock().await;
    load_record(home, id).rounds
}

async fn push_round(home: &Path, id: &str, round: RoundRecord) {
    let _g = file_lock().lock().await;
    let mut rec = load_record(home, id);
    // A re-run of the same round (after accepting a guard) replaces it.
    if round.round != 0 {
        rec.rounds.retain(|r| r.round != round.round);
    }
    rec.rounds.push(round);
    // Keep the baseline (round 0) and the last ten rounds.
    let baseline: Vec<RoundRecord> = rec
        .rounds
        .iter()
        .filter(|r| r.round == 0)
        .take(1)
        .cloned()
        .collect();
    let mut rest: Vec<RoundRecord> = rec.rounds.into_iter().filter(|r| r.round != 0).collect();
    if rest.len() > ROUNDS_KEPT {
        rest.drain(0..rest.len() - ROUNDS_KEPT);
    }
    rec.rounds = baseline.into_iter().chain(rest).collect();
    save_record(home, id, &rec);
}

// ---------------------------------------------------------------------------------------------
// Guards
// ---------------------------------------------------------------------------------------------

/// A stable hash of a guarded path: a file's bytes, or a folder's sorted file list with each
/// file's hash. "missing" when it does not exist.
pub fn hash_guard(root: &Path, rel: &str) -> Result<String, String> {
    let target = match crate::files::resolve(root, rel) {
        Ok(p) => p,
        Err(crate::files::FileError::NotFound(_)) => return Ok("missing".into()),
        Err(_) => return Err(format!("\"{rel}\" is not a path inside the workspace.")),
    };
    let meta = match std::fs::symlink_metadata(&target) {
        Ok(m) => m,
        Err(_) => return Ok("missing".into()),
    };
    if meta.file_type().is_symlink() {
        let link = std::fs::read_link(&target).unwrap_or_default();
        return Ok(format!("link:{}", link.display()));
    }
    if meta.is_file() {
        return hash_file(&target);
    }
    if meta.is_dir() {
        let mut entries: Vec<(String, String)> = Vec::new();
        let mut bytes = 0u64;
        walk(&target, &target, &mut entries, &mut bytes)
            .map_err(|_| format!("That folder is too large to guard: {rel}."))?;
        entries.sort();
        let mut h = Sha256::new();
        for (p, sum) in entries {
            h.update(p.as_bytes());
            h.update([0]);
            h.update(sum.as_bytes());
            h.update(b"\n");
        }
        return Ok(hex(&h.finalize()));
    }
    Ok("other".into())
}

fn walk(
    base: &Path,
    dir: &Path,
    out: &mut Vec<(String, String)>,
    bytes: &mut u64,
) -> Result<(), ()> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in read.flatten() {
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let rel = path
            .strip_prefix(base)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        if meta.file_type().is_symlink() {
            let link = std::fs::read_link(&path).unwrap_or_default();
            out.push((rel, format!("link:{}", link.display())));
        } else if meta.is_dir() {
            walk(base, &path, out, bytes)?;
        } else if meta.is_file() {
            *bytes += meta.len();
            if out.len() >= MAX_GUARD_FILES || *bytes > MAX_GUARD_BYTES {
                return Err(());
            }
            out.push((
                rel,
                hash_file(&path).unwrap_or_else(|_| "unreadable".into()),
            ));
        }
        if out.len() > MAX_GUARD_FILES {
            return Err(());
        }
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(hex(&Sha256::digest(&bytes)))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hash_guards(root: &Path, guards: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for g in guards {
        out.insert(g.clone(), hash_guard(root, g)?);
    }
    Ok(out)
}

/// Hash every check's guards at the start of a goal and store them.
pub async fn record_start_hashes(
    home: &Path,
    id: &str,
    goal: &Goal,
    root: &Path,
) -> Result<(), String> {
    let root = root.to_path_buf();
    let checks = goal.checks.clone();
    let hashes = tokio::task::spawn_blocking(move || {
        let mut all = BTreeMap::new();
        for c in &checks {
            all.insert(c.id.clone(), hash_guards(&root, &c.guards)?);
        }
        Ok::<_, String>(all)
    })
    .await
    .map_err(|_| "Could not hash the guarded paths.".to_string())??;
    let _g = file_lock().lock().await;
    let mut rec = load_record(home, id);
    rec.hashes = hashes;
    save_record(home, id, &rec);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Running one check
// ---------------------------------------------------------------------------------------------

pub struct CommandRun {
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub output: String,
}

fn check_env() -> Vec<(String, String)> {
    let drop = [
        "CLAUDECODE",
        "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_CODE_SSE_PORT",
        "CURSOR_AGENT",
        "CODEX_SANDBOX",
        "CODEX_SANDBOX_NETWORK_DISABLED",
    ];
    std::env::vars()
        .filter(|(k, _)| !k.starts_with("SYNCAIDYNC") && !drop.contains(&k.as_str()))
        .collect()
}

fn kill_group(pid: u32) {
    let _ = std::process::Command::new("/bin/kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Run `command` with `/bin/sh -c` in `root`, in its own process group, stdout and stderr
/// together, keeping the last 64 KiB. Anything it leaves running is killed afterwards.
pub async fn run_command(root: &Path, command: &str, timeout_s: u32) -> CommandRun {
    let started = Instant::now();
    let script = format!("exec 2>&1\n{command}");
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(script)
        .current_dir(root)
        .env_clear()
        .envs(check_env())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .kill_on_drop(true);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return CommandRun {
                passed: false,
                exit_code: None,
                timed_out: false,
                duration_ms: started.elapsed().as_millis() as u64,
                output: format!("SyncAiDync could not start the check: {e}"),
            }
        }
    };
    let pid = child.id();
    let mut stdout = child.stdout.take();
    let reader = tokio::spawn(async move {
        let mut kept: Vec<u8> = Vec::new();
        let Some(out) = stdout.as_mut() else {
            return kept;
        };
        let mut buf = [0u8; 8192];
        loop {
            match out.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    kept.extend_from_slice(&buf[..n]);
                    if kept.len() > OUTPUT_KEEP * 2 {
                        let cut = kept.len() - OUTPUT_KEEP;
                        kept.drain(0..cut);
                    }
                }
            }
        }
        kept
    });
    let limit = Duration::from_secs(u64::from(timeout_s.max(1)));
    let waited = tokio::time::timeout(limit, child.wait()).await;
    let (exit_code, timed_out) = match waited {
        Ok(Ok(status)) => (status.code(), false),
        Ok(Err(_)) => (None, false),
        Err(_) => (None, true),
    };
    if let Some(pid) = pid {
        kill_group(pid);
    }
    if timed_out {
        let _ = child.kill().await;
    }
    let mut kept = match tokio::time::timeout(Duration::from_secs(3), reader).await {
        Ok(Ok(k)) => k,
        _ => Vec::new(),
    };
    if kept.len() > OUTPUT_KEEP {
        let cut = kept.len() - OUTPUT_KEEP;
        kept.drain(0..cut);
    }
    let mut output = strip_ansi(&String::from_utf8_lossy(&kept));
    if timed_out {
        output.push_str(&format!(
            "\n(SyncAiDync stopped the check after {timeout_s} s.)"
        ));
    }
    CommandRun {
        passed: !timed_out && exit_code == Some(0),
        exit_code,
        timed_out,
        duration_ms: started.elapsed().as_millis() as u64,
        output,
    }
}

/// Run a goal's command checks. With `staged`, full checks are skipped when a quick check
/// fails. Then compare each check's guards to the stored hashes.
async fn run_checks(
    reg: &Registry,
    id: &str,
    root: &Path,
    goal: &Goal,
    staged: bool,
) -> Vec<CheckOutcome> {
    let _permit = semaphore().acquire().await;
    let mut outcomes: Vec<CheckOutcome> = Vec::new();
    let commands: Vec<_> = goal.checks.iter().filter(|c| c.is_command()).collect();
    let order = commands
        .iter()
        .filter(|c| c.stage == CheckStage::Quick)
        .chain(commands.iter().filter(|c| c.stage == CheckStage::Full));
    let mut quick_failed = false;
    for c in order {
        if staged && quick_failed && c.stage == CheckStage::Full {
            outcomes.push(CheckOutcome {
                check: c.id.clone(),
                passed: false,
                exit_code: None,
                timed_out: false,
                tampered: false,
                skipped: true,
                duration_ms: 0,
                changed: vec![],
                created: vec![],
                output: String::new(),
            });
            continue;
        }
        reg.set_goal_running(id, Some(c.id.clone())).await;
        let run = run_command(root, c.command.as_deref().unwrap_or("false"), c.timeout_s).await;
        if c.stage == CheckStage::Quick && !run.passed {
            quick_failed = true;
        }
        outcomes.push(CheckOutcome {
            check: c.id.clone(),
            passed: run.passed,
            exit_code: run.exit_code,
            timed_out: run.timed_out,
            tampered: false,
            skipped: false,
            duration_ms: run.duration_ms,
            changed: vec![],
            created: vec![],
            output: run.output,
        });
    }
    reg.set_goal_running(id, None).await;

    // Guards: compare to the reference hashes.
    let reference = {
        let _g = file_lock().lock().await;
        load_record(&reg.home_path(), id).hashes
    };
    let root = root.to_path_buf();
    let checks = goal.checks.clone();
    // Per check: the guards that changed, and those of them that did not exist before.
    type Changed = BTreeMap<String, (Vec<String>, Vec<String>)>;
    let changed: Changed = tokio::task::spawn_blocking(move || {
        let mut out = BTreeMap::new();
        for c in &checks {
            let Some(want) = reference.get(&c.id) else {
                continue;
            };
            let mut paths = Vec::new();
            let mut created = Vec::new();
            for g in &c.guards {
                let now = hash_guard(&root, g).unwrap_or_else(|_| "outside".into());
                if want.get(g) != Some(&now) {
                    paths.push(g.clone());
                    if want.get(g).map(String::as_str) == Some("missing") {
                        created.push(g.clone());
                    }
                }
            }
            if !paths.is_empty() {
                out.insert(c.id.clone(), (paths, created));
            }
        }
        out
    })
    .await
    .unwrap_or_default();
    for o in &mut outcomes {
        if let Some((paths, created)) = changed.get(&o.check) {
            o.tampered = true;
            o.changed = paths.clone();
            o.created = created.clone();
        }
    }
    // A manual check whose guards changed still needs accepting.
    for c in goal.checks.iter().filter(|c| !c.is_command()) {
        if let Some((paths, created)) = changed.get(&c.id) {
            outcomes.push(CheckOutcome {
                check: c.id.clone(),
                passed: false,
                exit_code: None,
                timed_out: false,
                tampered: true,
                skipped: false,
                duration_ms: 0,
                changed: paths.clone(),
                created: created.clone(),
                output: String::new(),
            });
        }
    }
    outcomes
}

fn stored(outcomes: &[CheckOutcome]) -> Vec<CheckOutcome> {
    outcomes
        .iter()
        .map(|o| {
            let mut o = o.clone();
            if o.output.len() > OUTPUT_STORE {
                let mut start = o.output.len() - OUTPUT_STORE;
                while !o.output.is_char_boundary(start) {
                    start += 1;
                }
                o.output = format!("…\n{}", &o.output[start..]);
            }
            o
        })
        .collect()
}

fn apply_results(goal: &mut Goal, outcomes: &[CheckOutcome], at: u64) {
    for c in &mut goal.checks {
        if let Some(o) = outcomes.iter().find(|o| o.check == c.id) {
            if c.is_command() {
                c.last = Some(o.result(at));
            } else if o.tampered {
                let mut r = c.last.clone().unwrap_or(goal::CheckResult {
                    passed: false,
                    exit_code: None,
                    timed_out: false,
                    tampered: false,
                    skipped: false,
                    duration_ms: 0,
                    at,
                });
                r.tampered = true;
                r.at = at;
                c.last = Some(r);
            }
        }
    }
    goal.running = None;
}

// ---------------------------------------------------------------------------------------------
// Driving the session
// ---------------------------------------------------------------------------------------------

/// The baseline, then the agent: runs every command check once, stores the results, and starts
/// the agent with the brief as its first prompt.
pub async fn baseline_then_start(reg: Arc<Registry>, id: String) {
    let Some(session) = reg.get(&id).await else {
        return;
    };
    let Some(goal) = session.goal.clone() else {
        return;
    };
    let root = PathBuf::from(&session.workspace.path);
    let outcomes = run_checks(&reg, &id, &root, &goal, false).await;
    let at = now_ms();
    push_round(
        &reg.home_path(),
        &id,
        RoundRecord {
            round: 0,
            at,
            results: stored(&outcomes),
        },
    )
    .await;
    let _ = reg
        .update_goal(&id, |s| {
            let Some(g) = s.goal.as_mut() else {
                return Ok(());
            };
            g.baseline = outcomes
                .iter()
                .filter(|o| !o.skipped)
                .map(|o| goal::BaselineEntry {
                    check: o.check.clone(),
                    passed: o.passed,
                })
                .collect();
            apply_results(g, &outcomes, at);
            if g.status == GoalStatus::Baseline {
                g.status = GoalStatus::Working;
            }
            Ok(())
        })
        .await;
    if let Err(e) = reg.start_after_baseline(&id).await {
        let _ = reg
            .update_goal(&id, |s| {
                if let Some(g) = s.goal.as_mut() {
                    goal::pause(
                        g,
                        goal::PauseReason::Ended,
                        format!("The agent could not start: {}", e.message),
                    );
                }
                Ok(())
            })
            .await;
    }
}

/// The first prompt to give the agent for a goal session.
pub fn brief_for(session: &Session) -> Option<String> {
    session
        .goal
        .as_ref()
        .map(|g| compose_brief(session.prompt.as_deref(), g))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// The agent ended a turn, or the person pressed Keep going.
    Turn,
    /// "Run checks now".
    Now,
    /// The person accepted the last changed guard: run the same round's checks again.
    Recheck,
}

/// Run a round of checks and act on the result.
pub async fn run_round(
    reg: Arc<Registry>,
    id: String,
    trigger: Trigger,
) -> Result<Session, CreateError> {
    // Claim the goal for checking.
    let (session, refresh_only) = reg
        .update_goal(&id, |s| {
            let g = s
                .goal
                .as_mut()
                .ok_or_else(|| CreateError::not_found("This session has no goal."))?;
            match (trigger, g.status) {
                (_, GoalStatus::Checking | GoalStatus::Reviewing | GoalStatus::Baseline) => Err(
                    CreateError::conflict("The checks are already running for this goal."),
                ),
                (Trigger::Turn, GoalStatus::Working) | (Trigger::Now, GoalStatus::Working) => {
                    g.status = GoalStatus::Checking;
                    g.round += 1;
                    g.paused = None;
                    Ok(false)
                }
                (Trigger::Recheck, GoalStatus::Paused)
                    if g.paused
                        .as_ref()
                        .is_some_and(|p| p.reason == goal::PauseReason::Guard) =>
                {
                    g.status = GoalStatus::Checking;
                    g.paused = None;
                    Ok(false)
                }
                (Trigger::Recheck, _) => Err(CreateError::conflict(
                    "This goal is not waiting on a changed check.",
                )),
                (Trigger::Turn, _) => Err(CreateError::conflict("This goal is not working.")),
                (Trigger::Now, _) => Ok(true),
            }
        })
        .await?;
    let Some(goal_now) = session.goal.clone() else {
        return Ok(session);
    };
    let root = PathBuf::from(&session.workspace.path);
    let outcomes = run_checks(&reg, &id, &root, &goal_now, true).await;
    let at = now_ms();
    push_round(
        &reg.home_path(),
        &id,
        RoundRecord {
            round: goal_now.round,
            at,
            results: stored(&outcomes),
        },
    )
    .await;

    let (session, decision) = reg
        .update_goal(&id, |s| {
            let Some(g) = s.goal.as_mut() else {
                return Ok(None);
            };
            apply_results(g, &outcomes, at);
            if refresh_only || g.status != GoalStatus::Checking {
                return Ok(None);
            }
            let d = decide(g, &outcomes);
            match &d {
                Decision::Met => g.status = GoalStatus::Met,
                Decision::Review => g.status = GoalStatus::Reviewing,
                Decision::Round(_) => g.status = GoalStatus::Working,
                Decision::Pause(reason, message) => goal::pause(g, *reason, message.clone()),
            }
            Ok(Some(d))
        })
        .await?;
    match decision {
        Some(Decision::Round(text)) => {
            send_round(&reg, &id, &text).await;
        }
        Some(Decision::Met) => spawn_finish(&reg, &id),
        Some(Decision::Review) => {
            let reg2 = reg.clone();
            let id2 = id.clone();
            tokio::spawn(async move {
                run_review(reg2, id2, outcomes).await;
            });
        }
        _ => {}
    }
    Ok(reg.get(&id).await.unwrap_or(session))
}

async fn send_round(reg: &Arc<Registry>, id: &str, text: &str) {
    if reg.send(id, text, true).await.is_err() {
        let _ = reg
            .update_goal(id, |s| {
                if let Some(g) = s.goal.as_mut() {
                    goal::pause_for_end(g);
                }
                Ok(())
            })
            .await;
    }
}

async fn run_review(reg: Arc<Registry>, id: String, outcomes: Vec<CheckOutcome>) {
    let Some(session) = reg.get(&id).await else {
        return;
    };
    let Some(goal_now) = session.goal.clone() else {
        return;
    };
    let agent = goal_now
        .reviewer
        .agent
        .unwrap_or(if session.agent == AgentId::Shell {
            AgentId::Claude
        } else {
            session.agent
        });
    let changes = match &session.workspace.base_commit {
        Some(base_commit) => changes::gather_async(ChangeSource::Folder {
            root: PathBuf::from(&session.workspace.path),
            base_commit: base_commit.clone(),
            base_label: session
                .workspace
                .base
                .clone()
                .unwrap_or_else(|| base_commit.chars().take(7).collect()),
        })
        .await
        .ok(),
        None => None,
    };
    let prompt = build_review_prompt(&goal_now, &outcomes, changes.as_ref(), REVIEW_BUDGET);
    let model = goal_now.reviewer.model.clone();
    let reply = crate::oneshot::run(agent, model.as_deref(), &prompt).await;
    let verdict = match reply {
        Ok(text) => parse_review(&text, goal_now.round, now_ms()),
        Err(crate::oneshot::OneshotError::Unavailable(m))
        | Err(crate::oneshot::OneshotError::BadRequest(m)) => Err(m),
    };
    let decision = reg
        .update_goal(&id, |s| {
            let Some(g) = s.goal.as_mut() else {
                return Ok(None);
            };
            if g.status != GoalStatus::Reviewing {
                return Ok(None);
            }
            match &verdict {
                Err(m) => {
                    goal::pause(
                        g,
                        goal::PauseReason::Review,
                        format!("The reviewer could not run: {m}"),
                    );
                    Ok(None)
                }
                Ok(v) => {
                    g.verdict = Some(v.clone());
                    let d = decide_review(g, v);
                    match &d {
                        Decision::Met => g.status = GoalStatus::Met,
                        Decision::Round(_) => g.status = GoalStatus::Working,
                        Decision::Pause(reason, message) => {
                            goal::pause(g, *reason, message.clone())
                        }
                        Decision::Review => {}
                    }
                    Ok(Some(d))
                }
            }
        })
        .await;
    match decision {
        Ok((_, Some(Decision::Round(text)))) => send_round(&reg, &id, &text).await,
        Ok((_, Some(Decision::Met))) => spawn_finish(&reg, &id),
        _ => {}
    }
}

/// "Keep going": apply new limits, check the paused reason no longer holds, then run a round.
pub async fn continue_goal(
    reg: Arc<Registry>,
    id: String,
    limits: Option<Limits>,
) -> Result<Session, CreateError> {
    reg.update_goal(&id, |s| {
        let live = s.live;
        let cost = s.usage.as_ref().and_then(|u| u.cost_usd);
        let (state, since) = (s.state, s.state_since);
        let g = s
            .goal
            .as_mut()
            .ok_or_else(|| CreateError::not_found("This session has no goal."))?;
        if let Some(l) = &limits {
            goal::validate_limits(l).map_err(CreateError::unprocessable)?;
        }
        let mut trial = g.clone();
        if let Some(l) = &limits {
            trial.limits = l.clone();
        }
        let working = working_ms_now(&trial, state, since, now_ms());
        goal::can_continue(&trial, live, cost, working).map_err(|m| {
            if m == "This goal is not paused." {
                CreateError::conflict(m)
            } else {
                CreateError::unprocessable(m)
            }
        })?;
        *g = trial;
        g.status = GoalStatus::Working;
        g.paused = None;
        Ok(())
    })
    .await?;
    run_round(reg, id, Trigger::Turn).await
}

/// Take the current state of a check's guards as the new reference; `None` takes every flagged
/// check. When that clears the last flag on a goal paused for it, the same round's checks run
/// again and the goal carries on from there.
pub async fn accept(
    reg: Arc<Registry>,
    id: String,
    check: Option<String>,
) -> Result<Session, CreateError> {
    let session = reg
        .get(&id)
        .await
        .ok_or_else(|| CreateError::not_found("No session with that id."))?;
    let goal_now = session
        .goal
        .as_ref()
        .ok_or_else(|| CreateError::not_found("This session has no goal."))?;
    let targets: Vec<goal::Check> = match &check {
        Some(want) => vec![goal_now
            .checks
            .iter()
            .find(|c| &c.id == want)
            .ok_or_else(|| CreateError::not_found("No check with that id."))?
            .clone()],
        None => goal_now
            .checks
            .iter()
            .filter(|c| c.last.as_ref().is_some_and(|l| l.tampered))
            .cloned()
            .collect(),
    };
    if targets.is_empty() {
        return Err(CreateError::conflict("No check is waiting to be accepted."));
    }
    let root = PathBuf::from(&session.workspace.path);
    let to_hash = targets.clone();
    let hashes = tokio::task::spawn_blocking(move || {
        let mut out = BTreeMap::new();
        for c in &to_hash {
            out.insert(c.id.clone(), hash_guards(&root, &c.guards)?);
        }
        Ok::<_, String>(out)
    })
    .await
    .map_err(|_| CreateError::internal("Could not hash the guarded paths."))?
    .map_err(CreateError::unprocessable)?;
    {
        let _g = file_lock().lock().await;
        let home = reg.home_path();
        let mut rec = load_record(&home, &id);
        rec.hashes.extend(hashes);
        save_record(&home, &id, &rec);
    }
    let ids: Vec<String> = targets.iter().map(|c| c.id.clone()).collect();
    let (s, recheck) = reg
        .update_goal(&id, |s| {
            let Some(g) = s.goal.as_mut() else {
                return Ok(false);
            };
            for c in g.checks.iter_mut().filter(|c| ids.contains(&c.id)) {
                if let Some(l) = c.last.as_mut() {
                    l.tampered = false;
                }
            }
            let waiting = g.status == GoalStatus::Paused
                && g.paused
                    .as_ref()
                    .is_some_and(|p| p.reason == goal::PauseReason::Guard);
            let flagged = g
                .checks
                .iter()
                .any(|c| c.last.as_ref().is_some_and(|l| l.tampered));
            Ok(waiting && !flagged)
        })
        .await?;
    if recheck {
        let reg2 = reg.clone();
        let id2 = id.clone();
        tokio::spawn(async move {
            let _ = run_round(reg2, id2, Trigger::Recheck).await;
        });
    }
    Ok(s)
}

/// Tick or untick a manual check. Ticking the last one on a goal paused for it meets the goal
/// when every command check last passed.
pub async fn set_manual(
    reg: Arc<Registry>,
    id: String,
    check: String,
    done: bool,
) -> Result<Session, CreateError> {
    let (s, met) = reg
        .update_goal(&id, |s| {
            let g = s
                .goal
                .as_mut()
                .ok_or_else(|| CreateError::not_found("This session has no goal."))?;
            let c = g
                .checks
                .iter_mut()
                .find(|c| c.id == check)
                .ok_or_else(|| CreateError::not_found("No check with that id."))?;
            if c.is_command() {
                return Err(CreateError::bad_request(
                    "Only manual checks can be ticked.",
                ));
            }
            c.done = done;
            let waiting = g.status == GoalStatus::Paused
                && g.paused
                    .as_ref()
                    .is_some_and(|p| p.reason == goal::PauseReason::Manual);
            let all_manual = g.checks.iter().filter(|c| !c.is_command()).all(|c| c.done);
            let commands_pass = g.checks.iter().filter(|c| c.is_command()).all(|c| {
                c.last
                    .as_ref()
                    .is_some_and(|l| l.passed && !l.tampered && !l.skipped)
            });
            if waiting && all_manual && commands_pass {
                g.status = GoalStatus::Met;
                g.paused = None;
                return Ok(true);
            }
            Ok(false)
        })
        .await?;
    if met {
        spawn_finish(&reg, &id);
    }
    Ok(s)
}

pub async fn drop_goal(reg: Arc<Registry>, id: String) -> Result<Session, CreateError> {
    let (s, _) = reg
        .update_goal(&id, |s| {
            let g = s
                .goal
                .as_mut()
                .ok_or_else(|| CreateError::not_found("This session has no goal."))?;
            g.status = GoalStatus::Dropped;
            g.paused = None;
            g.running = None;
            Ok(())
        })
        .await?;
    Ok(s)
}

/// Pause the goal and interrupt the agent when a cost or time limit is reached mid-turn.
pub async fn enforce_limits(reg: Arc<Registry>, id: String) {
    let hit = reg
        .update_goal(&id, |s| {
            let cost = s.usage.as_ref().and_then(|u| u.cost_usd);
            let (state, since) = (s.state, s.state_since);
            let agent = s.agent;
            let Some(g) = s.goal.as_mut() else {
                return Ok(None);
            };
            if g.status != GoalStatus::Working || state != syncaidync_core::State::Working {
                return Ok(None);
            }
            let working = working_ms_now(g, state, since, now_ms());
            if let Some((reason, message)) = over_limit(g, cost, working) {
                // Bank the working time up to now so the meter shows where it stopped.
                g.working_ms = working;
                goal::pause(g, reason, message);
                return Ok(Some(agent));
            }
            Ok(None)
        })
        .await;
    if let Ok((_, Some(agent))) = hit {
        if let Some(term) = reg.terminal(&id).await {
            let _ = term.write(interrupt_bytes(agent)).await;
        }
        // If the key did not end the turn, say so rather than implying the agent stopped.
        tokio::spawn(async move {
            tokio::time::sleep(interrupt_grace()).await;
            let _ = reg
                .update_goal(&id, |s| {
                    let still_working = s.state == syncaidync_core::State::Working;
                    if let Some(p) = s.goal.as_mut().and_then(|g| g.paused.as_mut()) {
                        if still_working && !p.message.contains(STILL_WORKING) {
                            p.message = format!("{} {STILL_WORKING}", p.message);
                        }
                    }
                    Ok(())
                })
                .await;
        });
    }
}

const STILL_WORKING: &str =
    "The agent may still be working: the interrupt key did not end its turn.";

/// How long to wait for the interrupt key to end a turn. Tests shorten it.
fn interrupt_grace() -> Duration {
    std::env::var("SYNCAIDYNC_INTERRUPT_GRACE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(10))
}

/// Summary of failing rounds per check, for close-out.
pub async fn takeaways_summary(home: &Path, session: &Session) -> Option<String> {
    let goal_now = session.goal.as_ref()?;
    let rounds = rounds(home, &session.id).await;
    let mut failed: Vec<(String, u32)> = Vec::new();
    for r in rounds.iter().filter(|r| r.round > 0) {
        for o in &r.results {
            if !o.passed && !o.skipped {
                match failed.iter_mut().find(|(id, _)| id == &o.check) {
                    Some((_, n)) => *n += 1,
                    None => failed.push((o.check.clone(), 1)),
                }
            }
        }
    }
    Some(goal::summary_for_takeaways(goal_now, &failed))
}

// ---------------------------------------------------------------------------------------------
// Draft context
// ---------------------------------------------------------------------------------------------

const CONTEXT_FILES: &[&str] = &[
    "README.md",
    "README",
    "readme.md",
    "AGENTS.md",
    "CLAUDE.md",
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "go.mod",
    "Makefile",
    "justfile",
];
const CONTEXT_FILE_CAP: usize = 8 * 1024;
const CONTEXT_SKILL_CAP: usize = 2 * 1024;
const CONTEXT_TOTAL: usize = 60 * 1024;
const CONTEXT_PATHS: usize = 400;

// ---------------------------------------------------------------------------------------------
// Finishing: the report, the commit and more work
// ---------------------------------------------------------------------------------------------

const TRANSCRIPT_FOR_FINISH: usize = 24_000;
const MAX_COMMIT_FILES: usize = 2000;

pub fn spawn_finish(reg: &Arc<Registry>, id: &str) {
    let reg = reg.clone();
    let id = id.to_string();
    tokio::spawn(async move { finish(reg, id).await });
}

async fn gather_changes(session: &Session) -> Option<syncaidync_core::ChangeSet> {
    let base_commit = session.workspace.base_commit.clone()?;
    changes::gather_async(ChangeSource::Folder {
        root: PathBuf::from(&session.workspace.path),
        base_label: session
            .workspace
            .base
            .clone()
            .unwrap_or_else(|| base_commit.chars().take(7).collect()),
        base_commit,
    })
    .await
    .ok()
}

/// Write the finish report for a met goal: the facts at once, then the note from a one-shot
/// call.
pub async fn finish(reg: Arc<Registry>, id: String) {
    let Some(session) = reg.get(&id).await else {
        return;
    };
    let Some(goal_now) = session.goal.clone() else {
        return;
    };
    if goal_now.status != GoalStatus::Met {
        return;
    }
    let changes = gather_changes(&session).await;
    let (files, more) = changes.as_ref().map(goal::report_files).unwrap_or_default();
    let mut report = goal::new_report(&goal_now, files, more, now_ms());
    // The agent may have committed its work itself: then there is nothing left to commit.
    let root = PathBuf::from(&session.workspace.path);
    if crate::git::is_repo(&root).await {
        let commits = commits_since(&root, session.workspace.base_commit.as_deref()).await;
        if !commits.is_empty() && matches!(uncommitted(&root).await, Ok(f) if f.is_empty()) {
            report.committed = commits[0].split(' ').next().map(str::to_string);
        }
    }
    let stamp = report.at;
    let placed = reg
        .update_goal(&id, |s| {
            let Some(g) = s.goal.as_mut() else {
                return Ok(false);
            };
            if g.status != GoalStatus::Met {
                return Ok(false);
            }
            g.report = Some(report);
            Ok(true)
        })
        .await;
    if !matches!(placed, Ok((_, true))) {
        return;
    }

    let home = reg.home_path();
    let s2 = session.clone();
    let transcript = tokio::task::spawn_blocking(move || {
        crate::takeaways::condensed(&s2, &home, TRANSCRIPT_FOR_FINISH)
    })
    .await
    .unwrap_or_default();
    let agent = if session.agent == AgentId::Shell {
        AgentId::Claude
    } else {
        session.agent
    };
    let prompt = goal::build_finish_prompt(
        &goal_now,
        changes.as_ref(),
        &transcript,
        goal::FINISH_BUDGET,
    );
    let reply = crate::oneshot::run(agent, session.model.as_deref(), &prompt).await;
    let parsed = match reply {
        Ok(text) => goal::parse_finish(&text),
        Err(crate::oneshot::OneshotError::Unavailable(m))
        | Err(crate::oneshot::OneshotError::BadRequest(m)) => Err(m),
    };
    let _ = reg
        .update_goal(&id, |s| {
            let Some(r) = s.goal.as_mut().and_then(|g| g.report.as_mut()) else {
                return Ok(());
            };
            if r.at != stamp {
                return Ok(());
            }
            match &parsed {
                Ok((summary, concerns, commit)) => {
                    r.state = goal::ReportState::Ready;
                    r.summary = summary.clone();
                    r.concerns = concerns.clone();
                    if !commit.is_empty() {
                        r.commit_message = commit.clone();
                    }
                    r.error = None;
                }
                Err(m) => {
                    r.state = goal::ReportState::Failed;
                    r.error = Some(m.clone());
                }
            }
            Ok(())
        })
        .await;
}

/// Write the report again (after the note failed).
pub async fn rewrite_report(reg: Arc<Registry>, id: String) -> Result<Session, CreateError> {
    let session = reg
        .get(&id)
        .await
        .ok_or_else(|| CreateError::not_found("No session with that id."))?;
    let g = session
        .goal
        .as_ref()
        .ok_or_else(|| CreateError::not_found("This session has no goal."))?;
    if g.status != GoalStatus::Met {
        return Err(CreateError::conflict("The goal is not met yet."));
    }
    if g.report
        .as_ref()
        .is_some_and(|r| r.state == goal::ReportState::Writing)
    {
        return Err(CreateError::conflict("The report is being written."));
    }
    spawn_finish(&reg, &id);
    Ok(session)
}

/// What a commit from the report would take in.
#[derive(Debug, Clone, Serialize)]
pub struct CommitPreview {
    pub branch: String,
    pub workspace: String,
    pub files: Vec<String>,
    /// Commits already on the branch since the session began (newest first), "hash subject".
    pub commits: Vec<String>,
}

/// Commits made in the workspace since the session began, newest first, as "hash subject".
async fn commits_since(root: &Path, base_commit: Option<&str>) -> Vec<String> {
    let Some(base) = base_commit else {
        return Vec::new();
    };
    let range = format!("{base}..HEAD");
    crate::git::run(root, &["log", "--format=%h %s", "-n", "20", &range])
        .await
        .map(|out| out.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

fn not_git() -> CreateError {
    CreateError::unprocessable(
        "This workspace is not a git repository, so there is nothing to commit.",
    )
}

/// Uncommitted paths in the workspace (tracked changes against HEAD and untracked files),
/// without the sidecar.
async fn uncommitted(root: &Path) -> Result<Vec<String>, CreateError> {
    if !crate::git::is_repo(root).await {
        return Err(not_git());
    }
    let tracked = crate::git::run_paths(
        root,
        &[
            "diff",
            "--relative",
            "--name-only",
            "--no-renames",
            "-z",
            "HEAD",
            "--",
            ".",
        ],
    )
    .await
    .map_err(|e| CreateError::unprocessable(e.message))?;
    let untracked = crate::git::run_paths(
        root,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".",
        ],
    )
    .await
    .map_err(|e| CreateError::unprocessable(e.message))?;
    let mut paths: Vec<_> = tracked
        .into_iter()
        .chain(untracked)
        .filter(|path| !goal::is_sidecar(path))
        .collect();
    paths.sort_unstable();
    paths.dedup();
    Ok(paths)
}

async fn met_session(reg: &Registry, id: &str) -> Result<Session, CreateError> {
    let session = reg
        .get(id)
        .await
        .ok_or_else(|| CreateError::not_found("No session with that id."))?;
    let g = session
        .goal
        .as_ref()
        .ok_or_else(|| CreateError::not_found("This session has no goal."))?;
    if g.status != GoalStatus::Met {
        return Err(CreateError::conflict("The goal is not met yet."));
    }
    Ok(session)
}

pub async fn commit_preview(reg: Arc<Registry>, id: String) -> Result<CommitPreview, CreateError> {
    let session = met_session(&reg, &id).await?;
    let root = PathBuf::from(&session.workspace.path);
    let files = uncommitted(&root).await?;
    let branch = crate::git::current_branch(&root).await.unwrap_or_default();
    let commits = commits_since(&root, session.workspace.base_commit.as_deref()).await;
    Ok(CommitPreview {
        branch,
        workspace: format!("{:?}", session.workspace.kind).to_lowercase(),
        files,
        commits,
    })
}

/// Commit every uncommitted change in the workspace (not the sidecar, not other staged work
/// outside those paths) with the person's message.
pub async fn commit(
    reg: Arc<Registry>,
    id: String,
    message: String,
) -> Result<Session, CreateError> {
    let _write = reg.goal_git_write.lock().await;
    let message = goal::clean_commit_message(&message);
    if message.is_empty() {
        return Err(CreateError::unprocessable("Write a commit message first."));
    }
    let session = met_session(&reg, &id).await?;
    let root = PathBuf::from(&session.workspace.path);
    let files = uncommitted(&root).await?;
    if files.is_empty() {
        return Err(CreateError::conflict("There is nothing to commit."));
    }
    if files.len() > MAX_COMMIT_FILES {
        return Err(CreateError::unprocessable(
            "Too many changed files to commit from here. Commit them in a terminal.",
        ));
    }
    let limit = Duration::from_secs(120);
    let mut add: Vec<&str> = vec!["--literal-pathspecs", "add", "-A", "--"];
    add.extend(files.iter().map(String::as_str));
    crate::git::run_timeout(&root, &add, limit)
        .await
        .map_err(|e| CreateError::unprocessable(e.message))?;
    let mut args: Vec<&str> = vec![
        "--literal-pathspecs",
        "commit",
        "--only",
        "-m",
        &message,
        "--",
    ];
    args.extend(files.iter().map(String::as_str));
    crate::git::run_timeout(&root, &args, limit)
        .await
        .map_err(|e| CreateError::unprocessable(format!("git commit failed: {}", e.message)))?;
    let short = crate::git::run(&root, &["rev-parse", "--short", "HEAD"])
        .await
        .unwrap_or_default();
    let (s, _) = reg
        .update_goal(&id, |s| {
            if let Some(r) = s.goal.as_mut().and_then(|g| g.report.as_mut()) {
                r.committed = Some(short.clone());
                r.commit_message = message.clone();
            }
            Ok(())
        })
        .await?;
    Ok(s)
}

/// What "Merge into <base>" would do for a worktree session.
#[derive(Debug, Clone, Serialize)]
pub struct MergePreview {
    pub repo: String,
    pub base: String,
    pub branch: String,
    /// Commits on the branch that the base does not have, newest first.
    pub commits: Vec<String>,
    /// Files changed in the worktree and not committed yet.
    pub uncommitted: usize,
    /// The branch checked out in the repository's own folder.
    pub checked_out: String,
    /// Why the merge cannot run now, as a sentence; null when it can.
    pub blocked: Option<String>,
}

async fn merge_head(repo: &Path) -> Result<Option<String>, CreateError> {
    let head = crate::git::run(repo, &["rev-parse", "--revs-only", "MERGE_HEAD"])
        .await
        .map_err(|e| CreateError::unprocessable(e.message))?;
    Ok((!head.is_empty()).then_some(head))
}

async fn merge_preview_for(session: &Session) -> Result<MergePreview, CreateError> {
    let ws = &session.workspace;
    if ws.kind != syncaidync_core::WorkspaceKind::Worktree {
        return Err(CreateError::unprocessable(
            "Only a session in its own worktree can be merged from here.",
        ));
    }
    let (Some(repo), Some(base), Some(branch)) = (&ws.repo, &ws.base, &ws.branch) else {
        return Err(CreateError::unprocessable(
            "This worktree does not record its repository, base and branch.",
        ));
    };
    let root = PathBuf::from(&ws.path);
    let repo_path = PathBuf::from(repo);
    if !crate::git::is_repo(&root).await || !crate::git::is_repo(&repo_path).await {
        return Err(not_git());
    }
    let uncommitted = uncommitted(&root).await?.len();
    let range = format!("{base}..{branch}");
    let commits: Vec<String> =
        crate::git::run(&repo_path, &["log", "--format=%h %s", "-n", "20", &range])
            .await
            .map(|out| out.lines().map(str::to_string).collect())
            .map_err(|e| CreateError::unprocessable(e.message))?;
    let checked_out = crate::git::current_branch(&repo_path)
        .await
        .unwrap_or_default();
    // A merge that stops on a conflict is aborted, and an abort cannot always bring back
    // uncommitted edits, so the repository folder must have none (untracked files are fine).
    let repo_dirty = !crate::git::run(
        &repo_path,
        &["status", "--porcelain", "--untracked-files=no"],
    )
    .await
    .map_err(|e| CreateError::unprocessable(e.message))?
    .is_empty();
    let on = if checked_out.is_empty() {
        "a detached HEAD".to_string()
    } else {
        checked_out.clone()
    };
    let blocked = if uncommitted > 0 {
        Some(format!(
            "{uncommitted} file{} in the worktree {} not committed yet. Commit first.",
            if uncommitted == 1 { "" } else { "s" },
            if uncommitted == 1 { "is" } else { "are" }
        ))
    } else if commits.is_empty() {
        Some(format!("{base} already has everything on {branch}."))
    } else if &checked_out != base {
        Some(format!(
            "The repository folder has {on} checked out, not {base}. Check out {base} there first."
        ))
    } else if merge_head(&repo_path).await?.is_some() {
        Some("The repository already has a merge in progress. Finish it in Git first.".into())
    } else if repo_dirty {
        Some(format!(
            "{repo} has uncommitted changes. Commit or stash them there first, so a merge \
             conflict cannot lose them."
        ))
    } else {
        None
    };
    Ok(MergePreview {
        repo: repo.clone(),
        base: base.clone(),
        branch: branch.clone(),
        commits,
        uncommitted,
        checked_out,
        blocked,
    })
}

pub async fn merge_preview(reg: Arc<Registry>, id: String) -> Result<MergePreview, CreateError> {
    let session = met_session(&reg, &id).await?;
    merge_preview_for(&session).await
}

/// Merge the session's branch into its base in the repository's own folder, with a merge
/// commit. A conflict is aborted and reported; nothing is pushed.
pub async fn merge(reg: Arc<Registry>, id: String) -> Result<Session, CreateError> {
    let _write = reg.goal_git_write.lock().await;
    let session = met_session(&reg, &id).await?;
    let p = merge_preview_for(&session).await?;
    if let Some(why) = p.blocked {
        return Err(CreateError::conflict(why));
    }
    let repo = PathBuf::from(&p.repo);
    let source = crate::git::rev_parse(&repo, &format!("refs/heads/{}", p.branch))
        .await
        .map_err(|e| CreateError::unprocessable(e.message))?;
    let goal_text = session
        .goal
        .as_ref()
        .map(|g| g.text.clone())
        .unwrap_or_default();
    let message = format!("Merge {}\n\n{}", p.branch, goal_text.trim());
    let limit = Duration::from_secs(120);
    let merged = crate::git::run_timeout(
        &repo,
        &["merge", "--no-ff", "--no-edit", "-m", &message, &source],
        limit,
    )
    .await;
    if let Err(e) = merged {
        let outcome = match merge_head(&repo).await {
            Ok(Some(head)) if head == source => match crate::git::run(&repo, &["merge", "--abort"])
                .await
            {
                Ok(_) => "Git aborted the merge.",
                Err(_) => "Git could not abort the merge. Check the repository before retrying.",
            },
            Ok(None) => "Git did not leave a merge in progress.",
            _ => "Check the repository for an in-progress merge before retrying.",
        };
        return Err(CreateError::unprocessable(format!(
            "The merge failed. {outcome} {}",
            e.message
        )));
    }
    let short = crate::git::run(&repo, &["rev-parse", "--short", "HEAD"])
        .await
        .unwrap_or_default();
    let (s, _) = reg
        .update_goal(&id, |s| {
            if let Some(r) = s.goal.as_mut().and_then(|g| g.report.as_mut()) {
                r.merged = Some(short.clone());
            }
            Ok(())
        })
        .await?;
    Ok(s)
}

/// "Keep working" on a met goal: send the person's instruction and check again after the
/// agent's turn, with room for more rounds.
pub async fn keep_working(
    reg: Arc<Registry>,
    id: String,
    text: String,
    rounds: Option<u32>,
) -> Result<Session, CreateError> {
    let text = text.trim().to_string();
    if text.is_empty() || text.chars().count() > 4000 {
        return Err(CreateError::unprocessable(
            "Say what else to do (at most 4000 characters).",
        ));
    }
    reg.update_goal(&id, |s| {
        let live = s.live;
        let g = s
            .goal
            .as_mut()
            .ok_or_else(|| CreateError::not_found("This session has no goal."))?;
        if g.status != GoalStatus::Met {
            return Err(CreateError::conflict("The goal is not met yet."));
        }
        if !live {
            return Err(CreateError::unprocessable("Resume the session first."));
        }
        let want = rounds.unwrap_or_else(|| (g.round + 2).min(10));
        if want <= g.round {
            return Err(CreateError::unprocessable(format!(
                "Allow more than the {} round{} already used.",
                g.round,
                if g.round == 1 { "" } else { "s" }
            )));
        }
        let mut limits = g.limits.clone();
        limits.rounds = want;
        goal::validate_limits(&limits).map_err(CreateError::unprocessable)?;
        g.limits = limits;
        g.status = GoalStatus::Working;
        g.report = None;
        g.verdict = None;
        g.paused = None;
        Ok(())
    })
    .await?;
    let note = format!(
        "{text}\n\n(SyncAiDync runs the goal's checks again after this turn. Do not change the \
         checks or the paths they rely on.)"
    );
    send_round(&reg, &id, &note).await;
    reg.get(&id)
        .await
        .ok_or_else(|| CreateError::not_found("No session with that id."))
}

/// Guards from a draft that do not exist in the workspace the session will use.
pub async fn missing_guards(
    workspace: &crate::registry::CreateWorkspace,
    guards: Vec<String>,
) -> Vec<String> {
    use crate::registry::CreateWorkspace;
    let mut out = Vec::new();
    match workspace {
        CreateWorkspace::Scratch {} => return guards,
        CreateWorkspace::Worktree { repo, base, .. } => {
            let repo = crate::workspaces::expand_tilde(repo);
            let Ok(top) = crate::git::toplevel(&repo).await else {
                return out;
            };
            for g in guards {
                let spec = format!("{base}:{g}");
                if crate::git::run(&top, &["cat-file", "-e", &spec])
                    .await
                    .is_err()
                {
                    out.push(g);
                }
            }
        }
        CreateWorkspace::Folder { path } => {
            let root = crate::workspaces::expand_tilde(path);
            for g in guards {
                let missing = match crate::files::resolve(&root, &g) {
                    Ok(p) => std::fs::symlink_metadata(p).is_err(),
                    Err(crate::files::FileError::NotFound(_)) => true,
                    Err(_) => false,
                };
                if missing {
                    out.push(g);
                }
            }
        }
    }
    out
}

/// Repository material for the draft prompt, from what the dialog chose.
pub async fn draft_context(
    workspace: &crate::registry::CreateWorkspace,
) -> Result<String, CreateError> {
    use crate::registry::CreateWorkspace;
    match workspace {
        CreateWorkspace::Scratch {} => Ok(String::new()),
        CreateWorkspace::Worktree { repo, base, .. } => {
            let repo = crate::workspaces::expand_tilde(repo);
            if !repo.is_dir() || !crate::git::is_repo(&repo).await {
                return Err(CreateError::bad_request(
                    "That folder is not a git repository.",
                ));
            }
            if base.starts_with('-') || base.trim().is_empty() {
                return Err(CreateError::bad_request("That base branch is not valid."));
            }
            let top = crate::git::toplevel(&repo)
                .await
                .map_err(|e| CreateError::unprocessable(e.message))?;
            let listing = crate::git::run(&top, &["ls-tree", "-r", "--name-only", base])
                .await
                .map_err(|e| CreateError::unprocessable(e.message))?;
            let paths: Vec<String> = listing.lines().map(str::to_string).collect();
            let mut files: Vec<(String, String)> = Vec::new();
            for p in wanted_paths(&paths) {
                let cap = if p.starts_with(".claude/skills/") {
                    CONTEXT_SKILL_CAP
                } else {
                    CONTEXT_FILE_CAP
                };
                if let Ok(text) = crate::git::run(&top, &["show", &format!("{base}:{p}")]).await {
                    files.push((p, cut_bytes(&text, cap)));
                }
            }
            Ok(render_context(&paths, &files))
        }
        CreateWorkspace::Folder { path } => {
            let root = crate::workspaces::expand_tilde(path);
            if !root.is_dir() {
                return Err(CreateError::bad_request("That path is not a folder."));
            }
            let root2 = root.clone();
            let (paths, files) = tokio::task::spawn_blocking(move || {
                let mut paths = Vec::new();
                list_two_levels(&root2, &root2, 0, &mut paths);
                let mut files = Vec::new();
                for p in wanted_paths(&paths) {
                    let cap = if p.starts_with(".claude/skills/") {
                        CONTEXT_SKILL_CAP
                    } else {
                        CONTEXT_FILE_CAP
                    };
                    if let Ok(target) = crate::files::resolve(&root2, &p) {
                        if let Ok(bytes) = std::fs::read(&target) {
                            files.push((p, cut_bytes(&String::from_utf8_lossy(&bytes), cap)));
                        }
                    }
                }
                (paths, files)
            })
            .await
            .map_err(|_| CreateError::internal("Could not read the folder."))?;
            Ok(render_context(&paths, &files))
        }
    }
}

fn wanted_paths(paths: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for name in CONTEXT_FILES {
        if paths.iter().any(|p| p == name) {
            out.push((*name).to_string());
        }
    }
    for p in paths {
        if p.starts_with(".claude/skills/")
            && p.ends_with("/SKILL.md")
            && p.matches('/').count() == 3
        {
            out.push(p.clone());
        }
    }
    out
}

fn list_two_levels(base: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
    if out.len() >= CONTEXT_PATHS {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        if out.len() >= CONTEXT_PATHS {
            return;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if name == ".git" || name == "node_modules" || name == "target" {
            continue;
        }
        let path = e.path();
        let rel = path
            .strip_prefix(base)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            let deeper = depth < 1 || rel.starts_with(".claude");
            if name.starts_with('.') && name != ".claude" && name != ".github" {
                continue;
            }
            out.push(format!("{rel}/"));
            if deeper && depth < 3 {
                list_two_levels(base, &path, depth + 1, out);
            }
        } else {
            out.push(rel);
        }
    }
}

fn render_context(paths: &[String], files: &[(String, String)]) -> String {
    let mut s = String::from("Files (first 400):\n");
    for p in paths.iter().take(CONTEXT_PATHS) {
        s.push_str(p);
        s.push('\n');
    }
    for (p, text) in files {
        if s.len() >= CONTEXT_TOTAL {
            break;
        }
        let room = CONTEXT_TOTAL.saturating_sub(s.len() + 40);
        s.push_str(&format!("\n=== {p} ===\n"));
        s.push_str(&cut_bytes(text, room));
        if !s.ends_with('\n') {
            s.push('\n');
        }
    }
    s
}

fn cut_bytes(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n(cut)", &s[..end])
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "syncaidync-goals-{name}-{}",
            crate::ids::random_hex(6)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[tokio::test]
    async fn command_passes_fails_and_combines_output() {
        let d = temp_dir("cmd");
        let ok = run_command(&d, "echo out; echo err >&2", 10).await;
        assert!(ok.passed);
        assert!(ok.output.contains("out") && ok.output.contains("err"));
        let bad = run_command(&d, "exit 3", 10).await;
        assert!(!bad.passed);
        assert_eq!(bad.exit_code, Some(3));
        let env = run_command(&d, "env", 10).await;
        assert!(!env.output.contains("SYNCAIDYNC"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn command_timeout_kills_the_group() {
        let d = temp_dir("timeout");
        let marker = d.join("child.pid");
        let cmd = format!("sleep 30 & echo $! > {}; wait", marker.display());
        let run = run_command(&d, &cmd, 1).await;
        assert!(run.timed_out && !run.passed);
        let pid = std::fs::read_to_string(&marker).unwrap();
        let alive = std::process::Command::new("/bin/kill")
            .args(["-0", pid.trim()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(!alive, "child sleep survived");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn guard_hashes_change_with_content() {
        let d = temp_dir("guard");
        std::fs::write(d.join("a.sh"), "one").unwrap();
        std::fs::create_dir_all(d.join("tests")).unwrap();
        std::fs::write(d.join("tests/x.rs"), "x").unwrap();
        let a1 = hash_guard(&d, "a.sh").unwrap();
        let t1 = hash_guard(&d, "tests").unwrap();
        std::fs::write(d.join("a.sh"), "two").unwrap();
        std::fs::write(d.join("tests/y.rs"), "y").unwrap();
        assert_ne!(a1, hash_guard(&d, "a.sh").unwrap());
        assert_ne!(t1, hash_guard(&d, "tests").unwrap());
        assert_eq!(hash_guard(&d, "nope/never").unwrap(), "missing");
        assert_eq!(hash_guard(&d, "gone.sh").unwrap(), "missing");
        assert!(hash_guard(&d, "../x").is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn wanted_paths_pick_known_files_and_skills() {
        let paths: Vec<String> = [
            "README.md",
            "src/main.rs",
            ".claude/skills/verify/SKILL.md",
            ".claude/skills/a/b/SKILL.md",
            "Cargo.toml",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            wanted_paths(&paths),
            vec!["README.md", "Cargo.toml", ".claude/skills/verify/SKILL.md"]
        );
    }
}
