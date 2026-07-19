use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config;

const MAX_LOG_LINES: usize = 200;
const FLAG_FILE: &str = "/dev/.tcp_module_log_cleared";

static LOG_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Guard to ensure one-time log clearing per boot
static CLEARED_ONCE: Mutex<bool> = Mutex::new(false);
static LOG_LOCK: Mutex<()> = Mutex::new(());

fn log_path() -> &'static PathBuf {
    LOG_PATH.get_or_init(|| config::module_dir().join("service.log"))
}

/// Ensure log is cleared on first run after boot
fn ensure_boot_cleared() {
    let Ok(mut cleared) = CLEARED_ONCE.lock() else {
        return;
    };
    if !*cleared {
        if fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(FLAG_FILE)
            .is_ok()
        {
            let _ = fs::remove_file(log_path());
        }
        *cleared = true;
    }
}

/// Rotate log if it exceeds MAX_LOG_LINES
fn rotate_if_needed() {
    let Ok(content) = fs::read_to_string(log_path()) else {
        return;
    };
    let line_count = content.lines().count();
    if line_count > MAX_LOG_LINES {
        // Keep the latter half
        let lines: Vec<&str> = content.lines().collect();
        let keep = &lines[lines.len() - (MAX_LOG_LINES / 2)..];
        let _ = fs::write(log_path(), keep.join("\n") + "\n");
    }
}

/// Format current timestamp for logging
fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    // Simple UTC timestamp: YYYY-MM-DD HH:MM:SS
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let mins = (time_secs % 3600) / 60;
    let secs_remain = time_secs % 60;

    // Calculate date from days since epoch (approximate but sufficient)
    let mut y = 1970u64;
    let mut d = days;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        y += 1;
    }

    let days_in_months: [u64; 12] = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut m = 0u64;
    for (i, &dim) in days_in_months.iter().enumerate() {
        if d < dim {
            m = i as u64;
            break;
        }
        d -= dim;
    }

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y,
        m + 1,
        d + 1,
        hours,
        mins,
        secs_remain
    )
}

fn is_leap(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

/// Log a message with timestamp
pub fn log_print(message: &str) {
    ensure_boot_cleared();

    let Ok(_guard) = LOG_LOCK.lock() else {
        return;
    };

    let entry = format!("{} - {}\n", timestamp(), message);

    let mut file = match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        Ok(f) => f,
        Err(_) => return,
    };

    let _ = file.write_all(entry.as_bytes());

    if config::module_dir().join("debug_mode").exists() {
        let debug_path = config::module_dir().join("debug.log");
        if let Ok(mut debug) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_path)
        {
            let _ = debug.write_all(
                format!("{} [PID:{}] {}\n", timestamp(), std::process::id(), message).as_bytes(),
            );
        }
    }

    rotate_if_needed();
}

/// Ensure boot-cleared flag is set (called by daemon on start)
pub fn ensure_flag() {
    ensure_boot_cleared();
}
