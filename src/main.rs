use clap::{Parser, Subcommand};
use std::process;

mod build_info;
mod config;
mod daemon;
mod install;
mod integrity;
mod kernel_module;
mod logging;
mod network;
mod policy;
mod profile;
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
        /// Include slower DNS and per-connection RTT/CWND diagnostics
        #[arg(long)]
        details: bool,
        /// Include full policy verification (tc/sysctl readback)
        #[arg(long)]
        verify: bool,
    },
    /// Sample network counters without running policy/proxy diagnostics
    Sample {
        /// Interface to sample; defaults to the active route interface
        #[arg(long)]
        iface: Option<String>,
        /// Include slower DNS and per-connection RTT/CWND diagnostics
        #[arg(long)]
        details: bool,
    },
    /// Reapply configured TCP policy without terminating existing connections
    Repair {
        /// Interface to repair; defaults to the active route interface
        #[arg(long)]
        iface: Option<String>,
    },
    /// Print fast JSON state for proxy-aware policy/UI integration
    Proxy,
    /// Print or refresh the install/runtime auto-tuning profile
    Profile {
        /// Re-detect the device/network and rewrite the managed auto profile
        #[arg(long)]
        refresh: bool,
        /// Enable or disable managed auto tuning (on/off)
        #[arg(long, value_name = "on|off")]
        auto: Option<String>,
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
        Command::Status {
            iface,
            runtime_only,
            details,
            verify,
        } => print_status(iface, runtime_only, details, verify),
        Command::Sample { iface, details } => print_sample(iface, details),
        Command::Repair { iface } => repair_policy(iface),
        Command::Proxy => print_proxy_status(),
        Command::Profile { refresh, auto } => print_profile(refresh, auto),
        Command::BuildInfo => print_build_info(),
        Command::VerifyModule { path } => integrity::verify_module(&path),
    };

    if let Err(e) = result {
        eprintln!("tcp_optimiser: error: {e}");
        process::exit(1);
    }
}

fn repair_policy(iface: Option<String>) -> std::io::Result<()> {
    let iface = iface.map(Ok).unwrap_or_else(network::active_iface)?;
    let record = policy::repair_policy(&iface)?;
    println!(
        "{}",
        serde_json::to_string(&record).map_err(std::io::Error::other)?
    );
    Ok(())
}

fn print_sample(iface: Option<String>, details: bool) -> std::io::Result<()> {
    let iface = iface.map(Ok).unwrap_or_else(network::fast_active_iface)?;
    let snapshot = stats::stats_snapshot(&iface, details)?;
    println!(
        "{}",
        serde_json::to_string(&snapshot).map_err(std::io::Error::other)?
    );
    Ok(())
}

fn print_proxy_status() -> std::io::Result<()> {
    let snapshot = proxy::detect_proxy_snapshot();
    println!(
        "{}",
        serde_json::to_string(&snapshot).map_err(std::io::Error::other)?
    );
    Ok(())
}

fn print_profile(refresh: bool, auto: Option<String>) -> std::io::Result<()> {
    let profile = if let Some(value) = auto {
        match value.as_str() {
            "on" | "true" | "1" => profile::set_auto_tuning(true)?,
            "off" | "false" | "0" => profile::set_auto_tuning(false)?,
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "--auto must be on or off",
                ))
            }
        }
    } else if refresh {
        profile::refresh_managed_profile()?.0
    } else {
        profile::load_or_refresh()?
    };
    println!(
        "{}",
        serde_json::to_string(&profile).map_err(std::io::Error::other)?
    );
    Ok(())
}

fn print_build_info() -> std::io::Result<()> {
    println!(
        "{}",
        serde_json::to_string(&build_info::current()).map_err(std::io::Error::other)?
    );
    Ok(())
}

fn print_status(
    iface: Option<String>,
    runtime_only: bool,
    details: bool,
    verify: bool,
) -> std::io::Result<()> {
    let iface = iface.map(Ok).unwrap_or_else(network::fast_active_iface)?;
    let snapshot = stats::network_snapshot(&iface, !runtime_only, details, verify)?;
    println!(
        "{}",
        serde_json::to_string(&snapshot).map_err(std::io::Error::other)?
    );
    Ok(())
}
