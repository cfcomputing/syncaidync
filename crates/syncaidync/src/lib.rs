//! SyncAiDync server library (used by the binary and integration tests).

pub mod agents_discover;
pub mod api;
pub mod changes;
pub mod config;
pub mod discovery;
pub mod files;
pub mod gh;
pub mod git;
pub mod goals;
pub mod hookcli;
pub mod http;
pub mod ids;
pub mod links;
pub mod load;
pub mod margins;
pub mod markdown;
pub mod oneshot;
pub mod pty;
pub mod pulls;
pub mod registry;
pub mod reviews;
pub mod store;
pub mod takeaways;
pub mod update;
pub mod workspaces;

use config::{Config, ConfigError};

#[derive(Debug)]
pub enum CliAction {
    Serve {
        port: Option<u16>,
        no_open: bool,
        parent_pid: Option<u32>,
    },
    Signal {
        agent: String,
    },
    Usage {
        agent: String,
    },
    Version,
    Help,
}

#[derive(Debug)]
pub enum ParseError {
    Message(String),
    ExitCode(i32),
}

pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<CliAction, ParseError> {
    let mut args = args.into_iter().peekable();
    // Skip argv[0] if present as a path-looking first arg when called from main with env::args.
    // Callers pass args after the binary name.
    let first = args.next();
    match first.as_deref() {
        None | Some("serve") => parse_serve(args),
        Some("--version") | Some("-V") => Ok(CliAction::Version),
        Some("--help") | Some("-h") => Ok(CliAction::Help),
        Some("signal") => {
            let agent = parse_agent_flag(&mut args)?;
            Ok(CliAction::Signal { agent })
        }
        Some("usage") => {
            let agent = parse_agent_flag(&mut args)?;
            Ok(CliAction::Usage { agent })
        }
        Some(flag) if flag.starts_with('-') => {
            // Allow `syncaidync --port N` as serve.
            let mut rest = vec![flag.to_string()];
            rest.extend(args);
            parse_serve(rest.into_iter().peekable())
        }
        Some(other) => Err(ParseError::Message(format!("unknown command: {other}"))),
    }
}

fn parse_serve(
    mut args: std::iter::Peekable<impl Iterator<Item = String>>,
) -> Result<CliAction, ParseError> {
    let mut port = None;
    let mut no_open = false;
    let mut parent_pid = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                let v = args
                    .next()
                    .ok_or_else(|| ParseError::Message("--port needs a value".into()))?;
                port = Some(
                    v.parse()
                        .map_err(|_| ParseError::Message(format!("invalid port: {v}")))?,
                );
            }
            "--no-open" => no_open = true,
            "--parent-pid" => {
                let v = args
                    .next()
                    .ok_or_else(|| ParseError::Message("--parent-pid needs a value".into()))?;
                parent_pid = Some(
                    v.parse()
                        .map_err(|_| ParseError::Message(format!("invalid pid: {v}")))?,
                );
            }
            "--help" | "-h" => return Ok(CliAction::Help),
            "--version" | "-V" => return Ok(CliAction::Version),
            other if other.starts_with('-') => {
                eprintln!("unknown flag: {other}");
                return Err(ParseError::ExitCode(2));
            }
            other => {
                return Err(ParseError::Message(format!("unexpected argument: {other}")));
            }
        }
    }
    Ok(CliAction::Serve {
        port,
        no_open,
        parent_pid,
    })
}

fn parse_agent_flag(
    args: &mut std::iter::Peekable<impl Iterator<Item = String>>,
) -> Result<String, ParseError> {
    match (args.next().as_deref(), args.next()) {
        (Some("--agent"), Some(agent)) => Ok(agent),
        _ => Err(ParseError::Message("expected: --agent <id>".into())),
    }
}

pub fn print_help() {
    println!(
        "\
SyncAiDync {version}

Usage:
  syncaidync [serve] [--port N] [--no-open] [--parent-pid PID]
  syncaidync signal --agent <id>
  syncaidync usage  --agent <id>
  syncaidync --version
  syncaidync --help

Local board for AI coding agents.",
        version = env!("CARGO_PKG_VERSION")
    );
}

pub async fn run_serve(
    port: Option<u16>,
    no_open: bool,
    parent_pid: Option<u32>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = Config::from_env(port, no_open, parent_pid).map_err(|e| {
        let ConfigError::Message(m) = e;
        std::io::Error::other(m)
    })?;
    let no_open = config.no_open;
    let server = match http::serve(config).await {
        Ok(s) => s,
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("Address already in use") || msg.contains("addr in use") {
                eprintln!("Port is in use. Is SyncAiDync already running? Try --port.");
                std::process::exit(1);
            }
            return Err(e);
        }
    };

    println!("SyncAiDync is listening on {}", server.addr);
    if !no_open {
        let url = format!("http://{}/#key={}", server.addr, server.key);
        open_browser(&url);
    }

    http::wait_for_ctrl_c(server).await;
    Ok(())
}

fn open_browser(url: &str) {
    let _ = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .env_remove("SYNCAIDYNC_KEY")
            .arg(url)
            .spawn()
    } else {
        std::process::Command::new("xdg-open")
            .env_remove("SYNCAIDYNC_KEY")
            .arg(url)
            .spawn()
    };
}
