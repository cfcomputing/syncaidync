//! Live process load: SyncAiDync itself and each live session's process group.
//!
//! Uses `ps` so we stay on the allowed crate list (no sysinfo). Snapshot only; nothing is stored.

use std::collections::HashMap;
use std::process::Command;

/// One process or process-group sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub cpu_pct: f64,
    pub rss_bytes: u64,
}

impl Sample {
    fn zero() -> Self {
        Self {
            cpu_pct: 0.0,
            rss_bytes: 0,
        }
    }

    fn add(&mut self, other: Sample) {
        self.cpu_pct += other.cpu_pct;
        self.rss_bytes = self.rss_bytes.saturating_add(other.rss_bytes);
    }
}

/// Host SyncAiDync process plus the sum of every live session's process group.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadSnapshot {
    pub host: Sample,
    pub agents: Sample,
    pub live_sessions: u32,
}

/// Read `ps` once and build a snapshot for this process and the given session leaders.
///
/// Each `pgid` is the PTY child's pid (session leader / process group id).
pub fn snapshot(session_pgids: &[u32]) -> LoadSnapshot {
    let rows = match read_ps() {
        Ok(rows) => rows,
        Err(_) => {
            return LoadSnapshot {
                host: Sample::zero(),
                agents: Sample::zero(),
                live_sessions: session_pgids.len() as u32,
            };
        }
    };

    let self_pid = std::process::id();
    let mut by_pid: HashMap<u32, Sample> = HashMap::new();
    let mut by_pgid: HashMap<u32, Sample> = HashMap::new();
    for row in rows {
        by_pid.insert(row.pid, row.sample);
        by_pgid
            .entry(row.pgid)
            .or_insert_with(Sample::zero)
            .add(row.sample);
    }

    let host = by_pid.get(&self_pid).copied().unwrap_or_else(Sample::zero);
    let mut agents = Sample::zero();
    let mut seen = std::collections::HashSet::new();
    for &pgid in session_pgids {
        if !seen.insert(pgid) {
            continue;
        }
        if let Some(s) = by_pgid.get(&pgid) {
            agents.add(*s);
        }
    }

    LoadSnapshot {
        host,
        agents,
        live_sessions: session_pgids.len() as u32,
    }
}

#[derive(Debug, Clone, Copy)]
struct PsRow {
    pid: u32,
    pgid: u32,
    sample: Sample,
}

fn read_ps() -> Result<Vec<PsRow>, String> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,pgid=,rss=,%cpu="])
        .output()
        .map_err(|e| format!("ps: {e}"))?;
    if !output.status.success() {
        return Err("ps exited with an error".into());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(parse_ps(&text))
}

/// Parse `ps -axo pid=,pgid=,rss=,%cpu=` lines. `rss` is kilobytes.
fn parse_ps(text: &str) -> Vec<PsRow> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let Ok(pid) = parts[0].parse::<u32>() else {
            continue;
        };
        let Ok(pgid) = parts[1].parse::<u32>() else {
            continue;
        };
        let Ok(rss_kb) = parts[2].parse::<u64>() else {
            continue;
        };
        let Ok(cpu_pct) = parts[3].parse::<f64>() else {
            continue;
        };
        rows.push(PsRow {
            pid,
            pgid,
            sample: Sample {
                cpu_pct,
                rss_bytes: rss_kb.saturating_mul(1024),
            },
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ps_lines() {
        let text = "\
  123  123  4096  1.5\n\
  124  123  2048  0.5\n\
  200  200   512 12.0\n\
bad line\n";
        let rows = parse_ps(text);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].pid, 123);
        assert_eq!(rows[0].pgid, 123);
        assert_eq!(rows[0].sample.rss_bytes, 4096 * 1024);
        assert!((rows[0].sample.cpu_pct - 1.5).abs() < f64::EPSILON);
        assert_eq!(rows[1].pgid, 123);
    }

    #[test]
    fn snapshot_sums_group() {
        // Live call: at least our own process should appear with some RSS.
        let snap = snapshot(&[]);
        assert!(snap.host.rss_bytes > 0, "host rss should be non-zero");
        assert_eq!(snap.live_sessions, 0);
        assert_eq!(snap.agents.rss_bytes, 0);
    }
}
