//! SyncAiDync: one local board for AI coding agents.

use syncaidync::{parse_args, print_help, run_serve, CliAction, ParseError};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(args) {
        Ok(CliAction::Version) => {
            println!("syncaidync {}", env!("CARGO_PKG_VERSION"));
        }
        Ok(CliAction::Help) => print_help(),
        Ok(CliAction::Serve {
            port,
            no_open,
            parent_pid,
        }) => {
            if let Err(e) = run_serve(port, no_open, parent_pid).await {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        Ok(CliAction::Signal { agent }) => {
            syncaidync::hookcli::run(syncaidync::hookcli::Kind::Signal, &agent);
        }
        Ok(CliAction::Usage { agent }) => {
            syncaidync::hookcli::run(syncaidync::hookcli::Kind::Usage, &agent);
        }
        Err(ParseError::ExitCode(code)) => {
            print_help();
            std::process::exit(code);
        }
        Err(ParseError::Message(m)) => {
            eprintln!("{m}");
            print_help();
            std::process::exit(2);
        }
    }
}
