use clap::{Parser, Subcommand};
use std::io;
use std::process;

mod baseline;
mod baseline_status;
mod build_info;
mod checkpoint;
mod config;
mod control;
mod daemon;
mod install;
mod integrity;
mod logging;
mod network;
mod plan;
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
    /// Preview the configured policy without writing kernel state
    Plan {
        /// Interface to evaluate; defaults to the active route interface
        #[arg(long)]
        iface: Option<String>,
    },
    /// Print only policy values that differ from the current kernel state
    Diff {
        /// Interface to evaluate; defaults to the active route interface
        #[arg(long)]
        iface: Option<String>,
    },
    /// Request a hot policy reload from the running daemon
    Reload,
    /// Pause all daemon policy writes while preserving monitoring
    Pause,
    /// Resume daemon policy writes and immediately reapply the configured policy
    Resume,
    /// Enter persistent read-only safe mode; pass --disable to leave it
    SafeMode {
        /// Leave safe mode and request an immediate policy reload
        #[arg(long)]
        disable: bool,
    },
    /// Print persistent runtime control mode and generation
    ControlStatus,
    /// Print the last-known-good runtime policy checkpoint
    CheckpointStatus,
    /// Enter safe mode and restore the last-known-good runtime policy
    RestoreCheckpoint,
    /// Capture the original managed kernel state without replacing an existing baseline
    CaptureBaseline,
    /// Read existing baseline health without creating or modifying rollback evidence
    BaselineStatus,
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
        Command::Plan { iface } => print_plan(iface),
        Command::Diff { iface } => print_diff(iface),
        Command::Reload => request_reload(),
        Command::Pause => pause_runtime(),
        Command::Resume => resume_runtime(),
        Command::SafeMode { disable } => set_safe_mode(disable),
        Command::ControlStatus => print_control_status(),
        Command::CheckpointStatus => print_checkpoint_status(),
        Command::RestoreCheckpoint => restore_checkpoint(),
        Command::CaptureBaseline => capture_baseline(),
        Command::BaselineStatus => print_baseline_status(),
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
    print_json(&record)?;
    if record.success {
        let mode = network::iface_mode(&iface);
        if mode != network::IfaceMode::Unknown {
            match daemon::resolve_policy(&iface, mode)
                .and_then(|policy| checkpoint::persist(&iface, mode, &policy).map(|_| ()))
            {
                Ok(()) => {}
                Err(error) => eprintln!(
                    "tcp_optimiser: warning: repair succeeded but checkpoint persistence failed: {error}"
                ),
            }
        }
    }
    Ok(())
}

fn print_plan(iface: Option<String>) -> io::Result<()> {
    let report = plan::build(iface)?;
    print_json(&report)
}

fn print_diff(iface: Option<String>) -> io::Result<()> {
    let report = plan::diff(iface)?;
    print_json(&report)
}

fn request_reload() -> io::Result<()> {
    let state = control::request_reload("cli-reload")?;
    print_json(&state)
}

fn pause_runtime() -> io::Result<()> {
    let state = control::pause("cli-pause")?;
    print_json(&state)
}

fn resume_runtime() -> io::Result<()> {
    let state = control::resume("cli-resume")?;
    print_json(&state)
}

fn set_safe_mode(disable: bool) -> io::Result<()> {
    let state = control::set_safe_mode(
        !disable,
        if disable {
            "cli-leave-safe-mode"
        } else {
            "cli-enter-safe-mode"
        },
    )?;
    print_json(&state)
}

fn print_control_status() -> io::Result<()> {
    match control::read() {
        Ok(state) => print_json(&state),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => print_json(
            &control::ControlState::safe_fallback(format!("invalid-control-state: {error}")),
        ),
        Err(error) => Err(error),
    }
}

fn print_checkpoint_status() -> io::Result<()> {
    print_json(&checkpoint::status()?)
}

fn restore_checkpoint() -> io::Result<()> {
    let report = checkpoint::restore()?;
    print_json(&report)?;
    if report.success {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "checkpoint restoration completed with {} error(s)",
            report.errors.len()
        )))
    }
}

fn capture_baseline() -> io::Result<()> {
    let summary = baseline::ensure_global_baseline()?;
    print_json(&summary)
}

fn print_baseline_status() -> io::Result<()> {
    print_json(&baseline_status::read()?)
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
