use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config;

const MAX_LOG_BYTES: u64 = 128 * 1024;
const KEEP_LOG_BYTES: u64 = 64 * 1024;
const FLAG_FILE: &str = "/dev/.tcp_module_log_cleared";

static LOG_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
static CLEARED_ONCE: Mutex<bool> = Mutex::new(false);
static LOG_LOCK: Mutex<()> = Mutex::new(());

fn log_path() -> &'static PathBuf {
    LOG_PATH.get_or_init(|| config::module_dir().join("service.log"))
}

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

/// Rotate only after the file crosses a byte threshold.
///
/// v3 read and split the entire log after *every* write. Policy switching emits
/// several lines in a burst, so that turned harmless logging into synchronous
/// storage churn on the hot path. v4 pays the read/copy cost only at rotation.
fn rotate_if_oversize() {
    let Ok(metadata) = fs::metadata(log_path()) else {
        return;
    };
    if metadata.len() <= MAX_LOG_BYTES {
        return;
    }

    let Ok(mut file) = fs::File::open(log_path()) else {
        return;
    };
    let keep = metadata.len().min(KEEP_LOG_BYTES);
    if file.seek(SeekFrom::End(-(keep as i64))).is_err() {
        return;
    }

    let mut tail = Vec::with_capacity(keep as usize);
    if file.read_to_end(&mut tail).is_err() {
        return;
    }

    // Avoid beginning the rotated log in the middle of a UTF-8/log line.
    let start = tail
        .iter()
        .position(|byte| *byte == b'\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let _ = fs::write(log_path(), &tail[start..]);
}

fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let mins = (time_secs % 3600) / 60;
    let secs_remain = time_secs % 60;

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

pub fn log_print(message: &str) {
    ensure_boot_cleared();

    let Ok(_guard) = LOG_LOCK.lock() else {
        return;
    };

    // Rotation is a rare threshold event now, never a per-line full-file read.
    rotate_if_oversize();

    let entry = format!("{} - {}\n", timestamp(), message);
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        let _ = file.write_all(entry.as_bytes());
    }

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
}

pub fn ensure_flag() {
    ensure_boot_cleared();
}
