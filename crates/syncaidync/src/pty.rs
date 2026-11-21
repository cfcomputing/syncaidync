//! PTY-backed terminal process with replay buffer and log.

use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use syncaidync_core::ByteRing;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;

const RING_CAP: usize = 256 * 1024;
const LOG_CAP: u64 = 16 * 1024 * 1024;

pub type OutputHook = Arc<dyn Fn(Bytes) + Send + Sync>;
pub type InputHook = Arc<dyn Fn(&[u8]) + Send + Sync>;
pub type ExitHook = Arc<dyn Fn(Option<i32>) + Send + Sync>;

pub struct Terminal {
    writer: Arc<Mutex<pty_process::OwnedWritePty>>,
    child: Arc<Mutex<Child>>,
    pub pid: Option<u32>,
    size: Arc<Mutex<(u16, u16)>>, // rows, cols
    ring: Arc<Mutex<ByteRing>>,
    log_path: PathBuf,
    out_tx: broadcast::Sender<Bytes>,
    reader: JoinHandle<()>,
    on_input: Option<InputHook>,
}

pub struct SpawnOpts {
    pub bin: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    pub rows: u16,
    pub cols: u16,
    pub log_path: PathBuf,
    /// Called with each output chunk (already pushed to ring/broadcast).
    pub on_output: Option<OutputHook>,
    /// Called with each write to the PTY (keystrokes / paste).
    pub on_input: Option<InputHook>,
    /// Called when the process exits.
    pub on_exit: Option<ExitHook>,
}

impl Terminal {
    pub async fn spawn(opts: SpawnOpts) -> Result<Self, String> {
        if let Some(parent) = opts.log_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("log dir {}: {e}", parent.display()))?;
        }
        let (pty, pts) = pty_process::open().map_err(|e| format!("open pty: {e}"))?;
        pty.resize(pty_process::Size::new(opts.rows, opts.cols))
            .map_err(|e| format!("resize: {e}"))?;

        let child = pty_process::Command::new(&opts.bin)
            .args(&opts.args)
            .current_dir(&opts.cwd)
            .envs(opts.env.iter().cloned())
            .env_remove("SYNCAIDYNC_KEY")
            .spawn(pts)
            .map_err(|e| format!("spawn {}: {e}", opts.bin))?;
        let pid = child.id();
        write_log_size(&opts.log_path, opts.rows, opts.cols).await;

        let (reader_half, writer_half) = pty.into_split();
        let writer = Arc::new(Mutex::new(writer_half));
        let child = Arc::new(Mutex::new(child));
        let size = Arc::new(Mutex::new((opts.rows, opts.cols)));
        let ring = Arc::new(Mutex::new(ByteRing::new(RING_CAP)));
        let (out_tx, _) = broadcast::channel(64);

        let ring_r = ring.clone();
        let out_tx_r = out_tx.clone();
        let child_r = child.clone();
        let log_path = opts.log_path.clone();
        let on_output = opts.on_output;
        let on_input = opts.on_input;
        let on_exit = opts.on_exit;

        let reader = tokio::spawn(async move {
            let mut reader_half = reader_half;
            let mut buf = [0u8; 8192];
            let mut log_len = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
            loop {
                match reader_half.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = Bytes::copy_from_slice(&buf[..n]);
                        {
                            let mut r = ring_r.lock().await;
                            if let Err(e) = append_log(&log_path, &chunk, &mut log_len).await {
                                eprintln!("syncaidync: terminal log: {e}");
                            }
                            r.push(&chunk);
                            let _ = out_tx_r.send(chunk.clone());
                        }
                        if let Some(cb) = &on_output {
                            cb(chunk);
                        }
                    }
                    Err(_) => break,
                }
            }
            let code = {
                let mut child = child_r.lock().await;
                match child.try_wait() {
                    Ok(Some(st)) => st.code(),
                    _ => match child.wait().await {
                        Ok(st) => st.code(),
                        Err(_) => None,
                    },
                }
            };
            if let Some(cb) = on_exit {
                cb(code);
            }
        });

        Ok(Self {
            writer,
            child,
            pid,
            size,
            ring,
            log_path: opts.log_path,
            out_tx,
            reader,
            on_input,
        })
    }

    pub async fn snapshot_and_subscribe(
        &self,
        full_history: bool,
    ) -> (Vec<u8>, broadcast::Receiver<Bytes>) {
        // The reader holds this lock while appending and broadcasting each chunk.
        let ring = self.ring.lock().await;
        let recent = ring.snapshot();
        let replay = if full_history {
            let log = read_log(&self.log_path, true).await;
            // Fall back to recent output if writing the log failed.
            if log.ends_with(&recent) {
                log
            } else {
                recent
            }
        } else {
            recent
        };
        (replay, self.out_tx.subscribe())
    }

    pub async fn snapshot(&self) -> Vec<u8> {
        self.ring.lock().await.snapshot()
    }

    pub async fn size(&self) -> (u16, u16) {
        *self.size.lock().await
    }

    pub async fn write(&self, data: &[u8]) -> Result<(), String> {
        if let Some(cb) = &self.on_input {
            cb(data);
        }
        let mut w = self.writer.lock().await;
        w.write_all(data)
            .await
            .map_err(|e| format!("pty write: {e}"))?;
        w.flush().await.map_err(|e| format!("pty flush: {e}"))?;
        Ok(())
    }

    pub async fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
        let w = self.writer.lock().await;
        w.resize(pty_process::Size::new(rows, cols))
            .map_err(|e| format!("pty resize: {e}"))?;
        *self.size.lock().await = (rows, cols);
        write_log_size(&self.log_path, rows, cols).await;
        Ok(())
    }

    /// SIGHUP the process group, then SIGKILL after 3 seconds if still alive.
    pub async fn stop(&self) {
        if let Some(pid) = self.pid {
            // SAFETY: child is a session leader (pty-process calls setsid); pid is the group id.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGHUP);
            }
            let child = self.child.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                let mut child = child.lock().await;
                if child.try_wait().ok().flatten().is_none() {
                    // SAFETY: escalate to SIGKILL on the same process group.
                    unsafe {
                        libc::kill(-(pid as i32), libc::SIGKILL);
                    }
                    let _ = child.wait().await;
                }
            });
        }
        self.reader.abort();
    }
}

pub async fn read_log(path: &std::path::Path, full_history: bool) -> Vec<u8> {
    let limit = if full_history {
        LOG_CAP
    } else {
        RING_CAP as u64
    };
    use tokio::io::{AsyncSeekExt, SeekFrom};
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return Vec::new();
    };
    let Ok(metadata) = file.metadata().await else {
        return Vec::new();
    };
    let start = metadata.len().saturating_sub(limit);
    if file.seek(SeekFrom::Start(start)).await.is_err() {
        return Vec::new();
    }
    let mut bytes = Vec::new();
    if file.take(limit).read_to_end(&mut bytes).await.is_err() {
        return Vec::new();
    }
    let first_char = bytes.iter().take_while(|byte| *byte & 0xc0 == 0x80).count();
    bytes.drain(..first_char);
    bytes
}

fn log_size_path(log: &std::path::Path) -> PathBuf {
    log.with_extension("size")
}

/// Remember the grid the log was drawn at, so an ended session replays at that size.
async fn write_log_size(log: &std::path::Path, rows: u16, cols: u16) {
    if let Err(e) = tokio::fs::write(log_size_path(log), format!("{rows} {cols}\n")).await {
        eprintln!("syncaidync: terminal size: {e}");
    }
}

/// The (rows, cols) last recorded for a log, if any.
pub async fn read_log_size(log: &std::path::Path) -> Option<(u16, u16)> {
    let text = tokio::fs::read_to_string(log_size_path(log)).await.ok()?;
    let mut parts = text
        .split_whitespace()
        .map(|n| n.parse::<u16>().ok().filter(|n| *n > 0));
    Some((parts.next()??, parts.next()??))
}

async fn append_log(path: &PathBuf, chunk: &[u8], log_len: &mut u64) -> Result<(), String> {
    use tokio::fs::OpenOptions;
    use tokio::io::AsyncWriteExt;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .map_err(|e| e.to_string())?;
    f.write_all(chunk).await.map_err(|e| e.to_string())?;
    // Tokio finishes file writes in the background; replay reads this log right after.
    f.flush().await.map_err(|e| e.to_string())?;
    *log_len += chunk.len() as u64;
    if *log_len > LOG_CAP {
        drop(f);
        trim_log(path, log_len).await?;
    }
    Ok(())
}

async fn trim_log(path: &PathBuf, log_len: &mut u64) -> Result<(), String> {
    let data = tokio::fs::read(path).await.map_err(|e| e.to_string())?;
    let keep = (data.len() / 2).max(1);
    let start = data.len() - keep;
    // Align to UTF-8 boundary.
    let mut start = start;
    while start < data.len() && (data[start] & 0xC0) == 0x80 {
        start += 1;
    }
    let replacement = path.with_extension("log.tmp");
    tokio::fs::write(&replacement, &data[start..])
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::rename(replacement, path)
        .await
        .map_err(|e| e.to_string())?;
    *log_len = (data.len() - start) as u64;
    Ok(())
}

/// Bracketed paste then optional carriage return (for POST …/send).
pub async fn send_text(term: &Terminal, text: &str, submit: bool) -> Result<(), String> {
    let cleaned: String = text.chars().filter(|c| *c != '\u{1b}').collect();
    let mut payload = Vec::new();
    payload.extend_from_slice(b"\x1b[200~");
    payload.extend_from_slice(cleaned.as_bytes());
    payload.extend_from_slice(b"\x1b[201~");
    term.write(&payload).await?;
    if submit {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        term.write(b"\r").await?;
    }
    Ok(())
}
