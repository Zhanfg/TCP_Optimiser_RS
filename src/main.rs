use clap::{Parser, Subcommand};
use std::process;

mod build_info;
mod config;
mod daemon;
mod install;
mod integrity;
mod logging;
mod network;
mod proxy;
mod stats;
mod sysctl;

/// TCP Optimiser — dynamic TCP congestion control for Android
#[derive(Parser)]
#[command(name = "tcp_optimiser", version, long_version = build_info::LONG_VERSION)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run as background daemon (replaces service.sh)
    Daemon,
    /// Run once and exit (replaces post-fs-data.sh)
    Once,
    /// Install/configure module (replaces customize.sh)
    Install,
    /// Print a JSON snapshot used for diagnostics and WebUI integration
    Status {
        /// Interface to sample; defaults to the active route interface
        #[arg(long)]
        iface: Option<String>,
    },
    /// Print build provenance embedded in this binary
    BuildInfo,
    /// Verify the signed module payload before installation
    VerifyModule {
        /// Extracted module staging directory
        path: std::path::PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Daemon => daemon::run(),
        Command::Once => daemon::run_once(),
        Command::Install => install::run(),
        Command::Status { iface } => print_status(iface),
        Command::BuildInfo => print_build_info(),
        Command::VerifyModule { path } => integrity::verify_module(&path),
    };

    if let Err(e) = result {
        eprintln!("tcp_optimiser: error: {e}");
        process::exit(1);
    }
}

fn print_build_info() -> std::io::Result<()> {
    println!(
        "{}",
        serde_json::to_string(&build_info::current()).map_err(std::io::Error::other)?
    );
    Ok(())
}

fn print_status(iface: Option<String>) -> std::io::Result<()> {
    let iface = iface.map(Ok).unwrap_or_else(network::active_iface)?;
    let snapshot = stats::network_snapshot(&iface)?;
    println!(
        "{}",
        serde_json::to_string(&snapshot).map_err(std::io::Error::other)?
    );
    Ok(())
}
