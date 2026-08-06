use clap::{Parser, Subcommand};
use std::io;
use std::process;

mod baseline;
mod build_info;
mod config;
mod daemon;
mod install;
mod integrity;
mod logging;
mod network;
mod policy;
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
        /// Skip throughput and connection statistics for lightweight UI refreshes
        #[arg(long)]
        runtime_only: bool,
    },
    /// Reapply configured TCP policy without terminating existing connections
    Repair {
        /// Interface to repair; defaults to the active route interface
        #[arg(long)]
        iface: Option<String>,
    },
    /// Capture the original managed kernel state without replacing an existing baseline
    CaptureBaseline,
    /// Restore the original managed kernel state and journaled interface qdiscs
    RestoreBaseline,
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
        Command::Status {
            iface,
            runtime_only,
        } => print_status(iface, runtime_only),
        Command::Repair { iface } => repair_policy(iface),
        Command::CaptureBaseline => capture_baseline(),
        Command::RestoreBaseline => restore_baseline(),
        Command::BuildInfo => print_build_info(),
        Command::VerifyModule { path } => integrity::verify_module(&path),
    };

    if let Err(e) = result {
        eprintln!("tcp_optimiser: error: {e}");
        process::exit(1);
    }
}

fn repair_policy(iface: Option<String>) -> io::Result<()> {
    let iface = iface.map(Ok).unwrap_or_else(network::active_iface)?;
    let record = policy::repair_policy(&iface)?;
    print_json(&record)
}

fn capture_baseline() -> io::Result<()> {
    let summary = baseline::ensure_global_baseline()?;
    print_json(&summary)
}

fn restore_baseline() -> io::Result<()> {
    let report = baseline::restore()?;
    print_json(&report)?;
    if report.success {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "baseline restoration completed with {} error(s)",
            report.errors.len()
        )))
    }
}

fn print_build_info() -> io::Result<()> {
    print_json(&build_info::current())
}

fn print_status(iface: Option<String>, runtime_only: bool) -> io::Result<()> {
    let iface = iface.map(Ok).unwrap_or_else(network::active_iface)?;
    let snapshot = stats::network_snapshot(&iface, !runtime_only)?;
    print_json(&snapshot)
}

fn print_json<T: serde::Serialize>(value: &T) -> io::Result<()> {
    println!(
        "{}",
        serde_json::to_string(value).map_err(io::Error::other)?
    );
    Ok(())
}
