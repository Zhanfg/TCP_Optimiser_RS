use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config;

const MAX_LOG_LINES: usize = 200;
const FLAG_FILE: &str = "/dev/.tcp_module_log_cleared";

static LOG_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Guard to ensure one-time log clearing per boot
static CLEARED_ONCE: Mutex<bool> = Mutex::new(false);

fn log_path() -> &'static PathBuf {
    LOG_PATH.get_or_init(|| config::module_dir().join("service.log"))
}

/// Ensure log is cleared on first run after boot
fn ensure_boot_cleared() {
    let mut cleared = CLEARED_ONCE.lock().unwrap();
    if !*cleared {
        if !std::path::Path::new(FLAG_FILE).exists() {
            let _ = fs::remove_file(log_path());
            let _ = fs::write(FLAG_FILE, "");
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
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

/// Log a message with timestamp
pub fn log_print(message: &str) {
    ensure_boot_cleared();

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

    rotate_if_needed();
}

/// Get the log flag file path
pub fn flag_file_exists() -> bool {
    std::path::Path::new(FLAG_FILE).exists()
}

/// Ensure boot-cleared flag is set (called by daemon on start)
pub fn ensure_flag() {
    ensure_boot_cleared();
}
