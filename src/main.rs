use std::process::ExitCode;

use clap::Parser;
use commitcrafter::cli::{Cli, Command};
use commitcrafter::error::Result;
use commitcrafter::log;
use tracing::{debug, info};

fn main() -> ExitCode {
    log::init_stderr();

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(err.exit_code())
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    debug!(?cli, "parsed CLI arguments");

    match cli.command {
        None => info!("(default) generate + commit — not yet implemented"),
        Some(Command::Setup(_)) => info!("setup — not yet implemented"),
        Some(Command::Init(_)) => info!("init — not yet implemented"),
        Some(Command::Doctor(_)) => info!("doctor — not yet implemented"),
        Some(Command::Config(_)) => info!("config — not yet implemented"),
        Some(Command::Providers(_)) => info!("providers — not yet implemented"),
        Some(Command::History(_)) => info!("history — not yet implemented"),
        Some(Command::Forget(_)) => info!("forget — not yet implemented"),
    }
    println!("commitcrafter: (no behavior yet — see issues #11–#70)");
    Ok(())
}
