//! Goals end to end: a fake Claude CLI, hook signals sent straight to the registry, and
//! checks run by the server in a real folder.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use common::{request, start_server, TestServer};
use serde_json::{json, Value};
use syncaidync::registry::Registry;
use syncaidync_core::{AgentId, Usage};
use tokio::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::const_new(());

fn write_script(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

/// A fake `claude`: one-shot runs (`-p`) print `oneshot.json`; interactive runs record their
/// arguments and everything typed into them.
fn fake_claude(dir: &Path) -> PathBuf {
    let script = dir.join("fake-claude");
    write_script(
        &script,
        &format!(
            "#!/bin/sh\n\
             if [ \"${{1:-}}\" = \"-p\" ]; then cat > /dev/null; cat \"{dir}/oneshot.json\"; exit 0; fi\n\
             printf '%s\\n' \"$@\" > \"{dir}/args.txt\"\n\
             stty raw -echo 2>/dev/null\n\
             exec cat > \"{dir}/typed.txt\"\n",
            dir = dir.display()
        ),
    );
    script
}

fn set_oneshot_reply(dir: &Path, reply: &Value) {
    let wrapped = json!({ "result": reply.to_string() });
    fs::write(dir.join("oneshot.json"), wrapped.to_string()).unwrap();
}

struct Ctx {
    ts: TestServer,
    dir: PathBuf,
    work: PathBuf,
}

impl Ctx {
    async fn new() -> Self {
        let ts = start_server().await;
        let dir = ts.home.join("fake");
        fs::create_dir_all(&dir).unwrap();
        let script = fake_claude(&dir);
        std::env::set_var("SYNCAIDYNC_BIN_CLAUDE", &script);
        let work = ts.home.join("work");
        fs::create_dir_all(&work).unwrap();
        Self { ts, dir, work }
    }

    fn reg(&self) -> Arc<Registry> {
        self.ts.server.state.registry.clone().unwrap()
    }

    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let bytes = body.map(|b| serde_json::to_vec(&b).unwrap());
        let auth = format!("Bearer {}", self.ts.key());
        let origin = format!("http://127.0.0.1:{}", self.ts.port());
        let (status, _, resp) = request(
            method,
            &format!("{}/api/v1{path}", self.ts.base()),
            &self.ts.host(),
            &[
                ("Authorization", auth.as_str()),
                ("Content-Type", "application/json"),
                ("Origin", origin.as_str()),
            ],
            bytes.as_deref(),
        )
        .await;
        let v = serde_json::from_slice(&resp).unwrap_or(Value::Null);
        (status, v)
    }

    async fn start(&self, goal: Value) -> String {
        let (status, v) = self
            .call(
                "POST",
                "/sessions",
                Some(json!({
                    "agent": "claude",
                    "prompt": "Make it work.",
                    "workspace": { "kind": "folder", "path": self.work.to_string_lossy() },
                    "goal": goal,
                })),
            )
            .await;
        assert_eq!(status, 201, "{v}");
        v["id"].as_str().unwrap().to_string()
    }

    async fn hook(&self, id: &str, event: &str) {
        self.reg()
            .apply_agent_payload(id, AgentId::Claude, &json!({ "hook_event_name": event }))
            .await
            .unwrap();
    }

    /// One agent turn: prompt submitted, then stopped.
    async fn turn(&self, id: &str) {
        self.hook(id, "UserPromptSubmit").await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        self.hook(id, "Stop").await;
    }

    async fn goal(&self, id: &str) -> Value {
        let s = self.reg().get(id).await.unwrap();
        serde_json::to_value(s.goal.unwrap()).unwrap()
    }

    async fn wait_goal(&self, id: &str, what: &str, f: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..200 {
            let g = self.goal(id).await;
            if f(&g) {
                return g;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("timed out waiting for {what}: {}", self.goal(id).await);
    }

    async fn typed(&self) -> String {
        for _ in 0..40 {
            if let Ok(t) = fs::read_to_string(self.dir.join("typed.txt")) {
                if !t.is_empty() {
                    return t;
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        String::new()
    }

    async fn wait_typed(&self, needle: &str) -> String {
        for _ in 0..80 {
            let t = fs::read_to_string(self.dir.join("typed.txt")).unwrap_or_default();
            if t.contains(needle) {
                return t;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!(
            "never typed {needle:?}: {:?}",
            fs::read_to_string(self.dir.join("typed.txt")).unwrap_or_default()
        );
    }
}

fn goal_json(checks: Value, rounds: u32) -> Value {
    json!({
        "text": "Create done.txt",
        "criteria": [{ "id": "k1", "text": "done.txt exists" }],
        "checks": checks,
        "limits": { "rounds": rounds, "cost_usd": null, "minutes": null },
        "baseline_on": false,
    })
}

fn done_check() -> Value {
    json!([{ "id": "c1", "criterion": "k1", "title": "File exists", "kind": "command",
             "command": "test -f done.txt", "guards": [], "timeout_s": 30 }])
}

#[tokio::test]
async fn goal_is_met_only_through_the_checks() {
    let _g = ENV_LOCK.lock().await;
    let cx = Ctx::new().await;
    let id = cx.start(goal_json(done_check(), 3)).await;

    // The agent got the brief as its first prompt.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let args = fs::read_to_string(cx.dir.join("args.txt")).unwrap();
    assert!(args.contains("Make it work."), "{args}");
    assert!(args.contains("## Goal (set in SyncAiDync)"), "{args}");
    assert!(args.contains("test -f done.txt"), "{args}");

    // Turn 1 does nothing: a round goes back to the agent.
    cx.turn(&id).await;
    let g = cx
        .wait_goal(&id, "round 1 sent", |g| {
            g["status"] == "working" && g["round"] == 1
        })
        .await;
    assert_eq!(g["checks"][0]["last"]["passed"], false);
    let typed = cx.wait_typed("0 of 1 pass. Round 1 of 3").await;
    assert!(typed.contains("File exists"));

    // Turn 2 creates the file: met.
    fs::write(cx.work.join("done.txt"), "ok").unwrap();
    cx.turn(&id).await;
    let g = cx.wait_goal(&id, "met", |g| g["status"] == "met").await;
    assert_eq!(g["round"], 2);
    assert_eq!(g["checks"][0]["last"]["passed"], true);

    // Evidence is stored with output.
    let (status, rec) = cx.call("GET", &format!("/sessions/{id}/goal"), None).await;
    assert_eq!(status, 200);
    assert_eq!(rec["rounds"].as_array().unwrap().len(), 2);
    cx.ts.shutdown().await;
}

#[tokio::test]
async fn round_limit_pauses_and_continue_needs_room() {
    let _g = ENV_LOCK.lock().await;
    let cx = Ctx::new().await;
    let id = cx.start(goal_json(done_check(), 1)).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    cx.turn(&id).await;
    let g = cx
        .wait_goal(&id, "paused", |g| g["status"] == "paused")
        .await;
    assert_eq!(g["paused"]["reason"], "rounds");

    let (status, v) = cx
        .call(
            "POST",
            &format!("/sessions/{id}/goal/continue"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 422, "{v}");
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Raise the round limit"));

    let (status, v) = cx
        .call(
            "POST",
            &format!("/sessions/{id}/goal/continue"),
            Some(json!({ "limits": { "rounds": 2, "cost_usd": null, "minutes": null } })),
        )
        .await;
    assert_eq!(status, 200, "{v}");
    let g = cx.wait_goal(&id, "second round", |g| g["round"] == 2).await;
    assert_eq!(g["status"], "paused", "{g}");
    assert_eq!(g["paused"]["reason"], "rounds");

    // Drop turns it back into a plain session.
    let (status, v) = cx
        .call("POST", &format!("/sessions/{id}/goal/drop"), None)
        .await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["goal"]["status"], "dropped");
    cx.ts.shutdown().await;
}

#[tokio::test]
async fn editing_a_guarded_file_never_counts_as_a_pass() {
    let _g = ENV_LOCK.lock().await;
    let cx = Ctx::new().await;
    fs::write(cx.work.join("check.sh"), "exit 1\n").unwrap();
    let checks = json!([{ "id": "c1", "criterion": "k1", "title": "Script", "kind": "command",
                          "command": "sh check.sh", "guards": ["check.sh"], "timeout_s": 30 }]);
    let id = cx.start(goal_json(checks, 3)).await;
    tokio::time::sleep(Duration::from_millis(150)).await;

    // The agent "fixes" it by editing the check itself.
    fs::write(cx.work.join("check.sh"), "exit 0\n").unwrap();
    cx.turn(&id).await;
    let g = cx
        .wait_goal(&id, "guard pause", |g| g["status"] == "paused")
        .await;
    assert_eq!(g["paused"]["reason"], "guard");
    assert_eq!(g["checks"][0]["last"]["tampered"], true);

    let (status, _) = cx
        .call(
            "POST",
            &format!("/sessions/{id}/goal/continue"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 422);

    let (status, v) = cx
        .call(
            "POST",
            &format!("/sessions/{id}/goal/accept"),
            Some(json!({ "check": "c1" })),
        )
        .await;
    assert_eq!(status, 200, "{v}");
    // Accepting the last changed check runs the same round again and carries on.
    let g = cx
        .wait_goal(&id, "met after accept", |g| g["status"] == "met")
        .await;
    assert_eq!(g["round"], 1);
    let (_, rec) = cx.call("GET", &format!("/sessions/{id}/goal"), None).await;
    assert_eq!(rec["rounds"].as_array().unwrap().len(), 1, "{rec}");
    cx.ts.shutdown().await;
}

#[tokio::test]
async fn cost_limit_interrupts_and_pauses() {
    let _g = ENV_LOCK.lock().await;
    let cx = Ctx::new().await;
    std::env::set_var("SYNCAIDYNC_INTERRUPT_GRACE_MS", "100");
    let mut goal = goal_json(done_check(), 3);
    goal["limits"]["cost_usd"] = json!(1.0);
    let id = cx.start(goal).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    cx.hook(&id, "UserPromptSubmit").await;
    cx.reg()
        .set_usage(
            &id,
            Usage {
                cost_usd: Some(1.25),
                context_pct: None,
                tokens: None,
            },
        )
        .await
        .unwrap();
    let g = cx
        .wait_goal(&id, "cost pause", |g| g["status"] == "paused")
        .await;
    assert_eq!(g["paused"]["reason"], "cost");
    let typed = cx.typed().await;
    assert!(typed.contains('\u{1b}'), "no interrupt key: {typed:?}");

    // The fake agent ignores the key and keeps "working": the paused message says so.
    let g = cx
        .wait_goal(&id, "still-working note", |g| {
            g["paused"]["message"]
                .as_str()
                .is_some_and(|m| m.contains("may still be working"))
        })
        .await;
    assert_eq!(g["paused"]["reason"], "cost");

    // The turn ending after the interrupt runs no checks.
    cx.hook(&id, "Stop").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(cx.goal(&id).await["round"], 0);
    cx.ts.shutdown().await;
}

#[tokio::test]
async fn baseline_runs_before_the_agent_starts() {
    let _g = ENV_LOCK.lock().await;
    let cx = Ctx::new().await;
    let mut goal = goal_json(done_check(), 3);
    goal["baseline_on"] = json!(true);
    let id = cx.start(goal).await;
    let g = cx
        .wait_goal(&id, "baseline done", |g| g["status"] == "working")
        .await;
    assert_eq!(g["baseline"][0]["passed"], false);
    for _ in 0..80 {
        if cx.reg().get(&id).await.unwrap().live && cx.dir.join("args.txt").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(cx.reg().get(&id).await.unwrap().live);
    let (_, rec) = cx.call("GET", &format!("/sessions/{id}/goal"), None).await;
    assert_eq!(rec["rounds"][0]["round"], 0);
    cx.ts.shutdown().await;
}

#[tokio::test]
async fn reviewer_concerns_become_a_round_then_approval_meets() {
    let _g = ENV_LOCK.lock().await;
    let cx = Ctx::new().await;
    let mut goal = goal_json(done_check(), 3);
    goal["reviewer"] = json!({ "on": true, "agent": "claude", "model": "fable" });
    fs::write(cx.work.join("done.txt"), "ok").unwrap();
    let id = cx.start(goal).await;
    tokio::time::sleep(Duration::from_millis(150)).await;

    set_oneshot_reply(
        &cx.dir,
        &json!({ "approved": false, "concerns": ["done.txt is empty of meaning."] }),
    );
    cx.turn(&id).await;
    cx.wait_typed("done.txt is empty of meaning.").await;
    let g = cx
        .wait_goal(&id, "working after concerns", |g| {
            g["status"] == "working" && g["verdict"]["approved"] == false
        })
        .await;
    assert_eq!(g["round"], 1);

    set_oneshot_reply(&cx.dir, &json!({ "approved": true, "concerns": [] }));
    cx.turn(&id).await;
    let g = cx.wait_goal(&id, "met", |g| g["status"] == "met").await;
    assert_eq!(g["verdict"]["approved"], true);
    cx.ts.shutdown().await;
}

#[tokio::test]
async fn manual_check_waits_for_the_person() {
    let _g = ENV_LOCK.lock().await;
    let cx = Ctx::new().await;
    let checks = json!([
        { "id": "c1", "criterion": "k1", "title": "File exists", "kind": "command",
          "command": "test -f done.txt", "guards": [], "timeout_s": 30 },
        { "id": "c2", "criterion": "k1", "title": "Open it and look", "kind": "manual",
          "guards": [], "timeout_s": 30 }
    ]);
    fs::write(cx.work.join("done.txt"), "ok").unwrap();
    let id = cx.start(goal_json(checks, 3)).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    cx.turn(&id).await;
    let g = cx
        .wait_goal(&id, "manual pause", |g| g["status"] == "paused")
        .await;
    assert_eq!(g["paused"]["reason"], "manual");
    let (status, v) = cx
        .call(
            "POST",
            &format!("/sessions/{id}/goal/manual"),
            Some(json!({ "check": "c2", "done": true })),
        )
        .await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["goal"]["status"], "met");
    cx.ts.shutdown().await;
}

#[tokio::test]
async fn draft_returns_checks_and_questions() {
    let _g = ENV_LOCK.lock().await;
    let cx = Ctx::new().await;
    fs::write(cx.work.join("README.md"), "Run `make test`.").unwrap();
    set_oneshot_reply(
        &cx.dir,
        &json!({
            "checks": [{ "criterion": "k1", "title": "Tests", "kind": "command", "stage": "quick",
                         "command": "make test", "guards": ["Makefile"], "why": "runs them" }],
            "questions": ["Which tests matter?"]
        }),
    );
    let (status, v) = cx
        .call(
            "POST",
            "/goals/draft",
            Some(json!({
                "agent": "claude",
                "text": "Tests pass",
                "criteria": [{ "id": "k1", "text": "make test passes" }],
                "workspace": { "kind": "folder", "path": cx.work.to_string_lossy() },
            })),
        )
        .await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["checks"][0]["command"], "make test");
    assert_eq!(v["checks"][0]["guards"][0], "Makefile");
    assert_eq!(v["questions"][0], "Which tests matter?");
    // The Makefile it guards does not exist yet, and the draft says so.
    assert_eq!(v["missing_guards"], json!(["Makefile"]));

    // Invalid goals are refused before anything starts.
    let (status, v) = cx
        .call(
            "POST",
            "/sessions",
            Some(json!({
                "agent": "claude",
                "workspace": { "kind": "folder", "path": cx.work.to_string_lossy() },
                "goal": goal_json(json!([]), 3),
            })),
        )
        .await;
    assert_eq!(status, 422, "{v}");
    cx.ts.shutdown().await;
}

fn finish_reply() -> Value {
    json!({
        "summary": "Added the file the goal asked for.",
        "concerns": ["Nothing reads done.txt yet."],
        "commit_message": "Add done.txt\n\nThe goal needed it."
    })
}

#[tokio::test]
async fn a_guard_the_agent_created_is_named_and_accept_all_carries_on() {
    let _g = ENV_LOCK.lock().await;
    let cx = Ctx::new().await;
    let checks = json!([{ "id": "c1", "criterion": "k1", "title": "Test", "kind": "command",
                          "command": "sh t.sh", "guards": ["t.sh"], "timeout_s": 30 }]);
    let id = cx.start(goal_json(checks, 3)).await;
    tokio::time::sleep(Duration::from_millis(150)).await;

    // The agent writes the test that judges it.
    fs::write(cx.work.join("t.sh"), "exit 0\n").unwrap();
    cx.turn(&id).await;
    let g = cx
        .wait_goal(&id, "guard pause", |g| g["status"] == "paused")
        .await;
    assert_eq!(g["paused"]["reason"], "guard");
    let m = g["paused"]["message"].as_str().unwrap();
    assert!(m.contains("The agent created t.sh"), "{m}");
    let (_, rec) = cx.call("GET", &format!("/sessions/{id}/goal"), None).await;
    assert_eq!(
        rec["rounds"][0]["results"][0]["created"][0], "t.sh",
        "{rec}"
    );

    set_oneshot_reply(&cx.dir, &finish_reply());
    let (status, v) = cx
        .call(
            "POST",
            &format!("/sessions/{id}/goal/accept"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 200, "{v}");
    let g = cx
        .wait_goal(&id, "report ready", |g| g["report"]["state"] == "ready")
        .await;
    assert_eq!(g["status"], "met");
    assert_eq!(g["round"], 1);
    assert_eq!(g["report"]["summary"], "Added the file the goal asked for.");
    assert_eq!(g["report"]["concerns"][0], "Nothing reads done.txt yet.");

    // Nothing left to accept.
    let (status, _) = cx
        .call(
            "POST",
            &format!("/sessions/{id}/goal/accept"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 409);
    cx.ts.shutdown().await;
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[tokio::test]
async fn a_met_goal_reports_commits_and_can_keep_working() {
    let _g = ENV_LOCK.lock().await;
    let cx = Ctx::new().await;
    git(&cx.work, &["init", "-q", "-b", "main"]);
    git(&cx.work, &["config", "user.email", "test@example.com"]);
    git(&cx.work, &["config", "user.name", "Test"]);
    fs::write(cx.work.join("README.md"), "hi\n").unwrap();
    git(&cx.work, &["add", "README.md"]);
    git(&cx.work, &["commit", "-q", "-m", "start"]);
    let id = cx.start(goal_json(done_check(), 3)).await;
    tokio::time::sleep(Duration::from_millis(150)).await;

    set_oneshot_reply(&cx.dir, &finish_reply());
    fs::write(cx.work.join("done.txt"), "ok").unwrap();
    fs::create_dir_all(cx.work.join(".syncaidync/margins")).unwrap();
    fs::write(cx.work.join(".syncaidync/margins/n.json"), "{}").unwrap();
    cx.turn(&id).await;
    let g = cx
        .wait_goal(&id, "report ready", |g| g["report"]["state"] == "ready")
        .await;
    let files: Vec<&str> = g["report"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(files, vec!["done.txt"], "{g}");
    assert!(g["report"]["commit_message"]
        .as_str()
        .unwrap()
        .starts_with("Add done.txt"));

    // A literal path that starts with a space or Git pathspec syntax must round-trip.
    let literal = [":(glob)*.txt", " leading-space.txt"];
    for name in literal {
        fs::write(cx.work.join(name), "literal").unwrap();
    }

    let (status, v) = cx
        .call("GET", &format!("/sessions/{id}/goal/commit"), None)
        .await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["branch"], "main");
    assert_eq!(v["files"], json!([literal[1], literal[0], "done.txt"]));

    let (status, v) = cx
        .call(
            "POST",
            &format!("/sessions/{id}/goal/commit"),
            Some(json!({ "message": "Add done.txt\n\nFor the goal." })),
        )
        .await;
    assert_eq!(status, 200, "{v}");
    assert!(v["goal"]["report"]["committed"]
        .as_str()
        .is_some_and(|h| !h.is_empty()));
    assert_eq!(
        git(&cx.work, &["log", "-1", "--format=%s"]).trim(),
        "Add done.txt"
    );
    for name in literal {
        assert_eq!(git(&cx.work, &["show", &format!("HEAD:{name}")]), "literal");
    }
    assert!(git(&cx.work, &["status", "--porcelain"]).contains(".syncaidync/"));
    let (status, _) = cx
        .call(
            "POST",
            &format!("/sessions/{id}/goal/commit"),
            Some(json!({ "message": "Again" })),
        )
        .await;
    assert_eq!(status, 409);

    // Keep working: the instruction goes to the agent and the goal drives again.
    let (status, v) = cx
        .call(
            "POST",
            &format!("/sessions/{id}/goal/more"),
            Some(json!({ "text": "Also write notes.txt." })),
        )
        .await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["goal"]["status"], "working");
    assert_eq!(v["goal"]["limits"]["rounds"], 3);
    assert!(v["goal"]["report"].is_null());
    cx.wait_typed("Also write notes.txt.").await;
    cx.ts.shutdown().await;
}

#[tokio::test]
async fn work_the_agent_committed_itself_shows_as_committed() {
    let _g = ENV_LOCK.lock().await;
    let cx = Ctx::new().await;
    git(&cx.work, &["init", "-q", "-b", "main"]);
    git(&cx.work, &["config", "user.email", "test@example.com"]);
    git(&cx.work, &["config", "user.name", "Test"]);
    fs::write(cx.work.join("README.md"), "hi\n").unwrap();
    git(&cx.work, &["add", "README.md"]);
    git(&cx.work, &["commit", "-q", "-m", "start"]);
    let id = cx.start(goal_json(done_check(), 3)).await;
    tokio::time::sleep(Duration::from_millis(150)).await;

    set_oneshot_reply(&cx.dir, &finish_reply());
    fs::write(cx.work.join("done.txt"), "ok").unwrap();
    git(&cx.work, &["add", "done.txt"]);
    git(&cx.work, &["commit", "-q", "-m", "Add done.txt myself"]);
    let head = git(&cx.work, &["rev-parse", "--short", "HEAD"]);
    cx.turn(&id).await;
    let g = cx
        .wait_goal(&id, "report ready", |g| g["report"]["state"] == "ready")
        .await;
    assert_eq!(g["report"]["committed"], head.trim(), "{g}");

    let (status, v) = cx
        .call("GET", &format!("/sessions/{id}/goal/commit"), None)
        .await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["files"], json!([]));
    assert_eq!(
        v["commits"],
        json!([format!("{} Add done.txt myself", head.trim())])
    );
    cx.ts.shutdown().await;
}

#[tokio::test]
async fn a_worktree_goal_merges_into_its_base() {
    let _g = ENV_LOCK.lock().await;
    let cx = Ctx::new().await;
    git(&cx.work, &["init", "-q", "-b", "main"]);
    git(&cx.work, &["config", "user.email", "test@example.com"]);
    git(&cx.work, &["config", "user.name", "Test"]);
    fs::write(cx.work.join("README.md"), "hi\n").unwrap();
    git(&cx.work, &["add", "README.md"]);
    git(&cx.work, &["commit", "-q", "-m", "start"]);
    let (status, v) = cx
        .call(
            "POST",
            "/sessions",
            Some(json!({
                "agent": "claude",
                "prompt": "Make it work.",
                "workspace": { "kind": "worktree", "repo": cx.work.to_string_lossy(),
                               "base": "main", "branch": "dync/done" },
                "goal": goal_json(done_check(), 3),
            })),
        )
        .await;
    assert_eq!(status, 201, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    let wt = PathBuf::from(v["workspace"]["path"].as_str().unwrap());
    tokio::time::sleep(Duration::from_millis(150)).await;

    set_oneshot_reply(&cx.dir, &finish_reply());
    fs::write(wt.join("done.txt"), "ok").unwrap();
    git(&wt, &["add", "done.txt"]);
    git(&wt, &["commit", "-q", "-m", "Add done.txt"]);
    cx.turn(&id).await;
    cx.wait_goal(&id, "report ready", |g| g["report"]["state"] == "ready")
        .await;

    // An unrelated merge can be in progress even with a clean worktree.
    git(&cx.work, &["checkout", "-qb", "other"]);
    git(&cx.work, &["commit", "-q", "--allow-empty", "-m", "other"]);
    git(&cx.work, &["checkout", "-q", "main"]);
    git(&cx.work, &["merge", "--no-ff", "--no-commit", "other"]);
    let (status, _) = cx
        .call("POST", &format!("/sessions/{id}/goal/merge"), None)
        .await;
    assert_eq!(status, 409);
    assert_eq!(
        git(&cx.work, &["rev-parse", "MERGE_HEAD"]),
        git(&cx.work, &["rev-parse", "other"])
    );
    git(&cx.work, &["merge", "--abort"]);

    // An edit in the main checkout blocks the merge.
    fs::write(cx.work.join("README.md"), "edited\n").unwrap();
    let (status, v) = cx
        .call("GET", &format!("/sessions/{id}/goal/merge"), None)
        .await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["commits"].as_array().unwrap().len(), 1, "{v}");
    assert!(
        v["blocked"]
            .as_str()
            .unwrap()
            .contains("uncommitted changes"),
        "{v}"
    );
    let (status, _) = cx
        .call("POST", &format!("/sessions/{id}/goal/merge"), None)
        .await;
    assert_eq!(status, 409);

    git(&cx.work, &["checkout", "--", "README.md"]);
    let (status, v) = cx
        .call("POST", &format!("/sessions/{id}/goal/merge"), None)
        .await;
    assert_eq!(status, 200, "{v}");
    assert!(v["goal"]["report"]["merged"]
        .as_str()
        .is_some_and(|h| !h.is_empty()));
    assert_eq!(
        git(&cx.work, &["log", "-1", "--format=%s"]).trim(),
        "Merge dync/done"
    );
    assert!(cx.work.join("done.txt").exists());
    let (_, v) = cx
        .call("GET", &format!("/sessions/{id}/goal/merge"), None)
        .await;
    assert!(
        v["blocked"]
            .as_str()
            .unwrap()
            .contains("already has everything"),
        "{v}"
    );
    cx.ts.shutdown().await;
}
