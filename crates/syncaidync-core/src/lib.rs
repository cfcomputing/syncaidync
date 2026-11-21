//! Pure logic for SyncAiDync.
//!
//! This crate has no I/O: no file system, no network, no process, no clock.
//! Callers pass in text and the current time.

#![forbid(unsafe_code)]

pub mod agents;
pub mod changemap;
pub mod diff;
pub mod goal;
pub mod imports;
pub mod json_extract;
pub mod model;
pub mod pulls;
pub mod readplan;
pub mod review;
pub mod ring;
pub mod signal;
pub mod startup;
pub mod takeaways;
pub mod time;
pub mod transcript;
pub mod version;

pub use agents::{
    adapter_for, claude_ids, shell_question, ActivityEvent, ActivityWatcher, Adapter, AgentId,
    ClaudeAdapter, CodexAdapter, CursorAdapter, LaunchPlan, LaunchRequest, Observes, OneShotPlan,
    PiAdapter, ShellAdapter,
};
pub use changemap::{
    build_map, file_digest, groups, hunk_id, language_for, stamp, untracked_file, ChangeBase,
    ChangeGroup, ChangeHead, ChangeMap, ChangeSet, MapEdge, MapNode, MAX_FILES, MAX_IMPORTER_NODES,
    MAX_LINES_PER_FILE, MAX_UNTRACKED, MAX_UNTRACKED_BYTES,
};
pub use diff::{parse as parse_diff, DiffLine, FileDiff, FileStatus, Hunk, LineKind};
pub use goal::{CheckOutcome, Decision, Goal, GoalStatus, NewGoal, PauseReason};
pub use json_extract::extract_json;
pub use model::{
    default_title, mark_ended, mark_launching, mark_startup_blocked, push_tool, set_title, Blocked,
    BlockedKind, NewSession, Session, State, ToolUse, Usage, Workspace, WorkspaceKind,
};
pub use pulls::{
    draft_payload, parse_ref, progress_for, remote_matches, Pull, PullProgress, PullRef,
};
pub use readplan::{
    build_prompt, parse_reply, Checkpoint, ReadPlan, DEFAULT_BUDGET as READPLAN_BUDGET,
};
pub use review::{Review, ReviewNote, MAX_NOTES, MAX_NOTE_CHARS, MAX_OVERALL_CHARS};
pub use ring::ByteRing;
pub use signal::{apply, Signal};
pub use startup::{find_marker, normalize_tail, strip_ansi, Marker};
pub use takeaways::{
    build_prompt as build_takeaways_prompt, destination as takeaway_destination,
    merge as merge_takeaways, normalize_takeaway, parse_reply as parse_takeaways_reply, DestCtx,
    Takeaway,
};
pub use transcript::{condense_claude, condense_terminal, DEFAULT_BUDGET as TRANSCRIPT_BUDGET};
