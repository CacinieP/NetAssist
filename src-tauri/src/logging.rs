//! Persistent logging: daily rolling files under the app config directory.
//!
//! Logs go to `<config_dir>/NetAssist/logs/netassist.log.YYYY-MM-DD` through a
//! non-blocking writer, so a slow disk never stalls the UI thread. Release
//! builds rely on this because packaged .app stdout is discarded; debug builds
//! additionally mirror to stdout for `tauri dev`. Files older than
//! [`LOG_RETENTION_DAYS`] are swept at startup and on each day rollover.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::RollingFileAppender;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::{Layer as _, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

/// Days of daily log files to keep. Logs are for troubleshooting, not
/// analytics — a week covers any realistic bug report window.
pub const LOG_RETENTION_DAYS: i64 = 7;
const LOG_PREFIX: &str = "netassist.log";

/// Log directory: `<config_dir>/NetAssist/logs` (same root as traffic data).
pub fn log_dir() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("NetAssist").join("logs"))
}

/// Initialize the tracing subscriber.
///
/// Level follows the existing convention: DEBUG in debug builds (process name
/// resolution), INFO in release — release previously ran DEBUG logs on hot
/// paths and it slowed things down. Falls back to stdout-only when the config
/// directory is unavailable or the log directory cannot be created (the
/// behavior before log persistence existed).
///
/// Returns the non-blocking writer guard. The caller must keep it alive for
/// the whole process — dropping it silently truncates file output.
pub fn init() -> Option<WorkerGuard> {
    let level = if cfg!(debug_assertions) {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    };

    let dir = log_dir().filter(|dir| fs::create_dir_all(dir).is_ok());
    let appender = dir.and_then(|dir| {
        RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix(LOG_PREFIX)
            .build(&dir)
            .ok()
    });

    let Some(appender) = appender else {
        tracing_subscriber::fmt().with_max_level(level).init();
        return None;
    };

    let (writer, guard) = tracing_appender::non_blocking(appender);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(writer)
        .with_filter(level);

    if cfg!(debug_assertions) {
        let stdout_layer = tracing_subscriber::fmt::layer().with_filter(level);
        tracing_subscriber::registry()
            .with(file_layer)
            .with(stdout_layer)
            .init();
    } else {
        tracing_subscriber::registry().with(file_layer).init();
    }
    Some(guard)
}

/// Delete daily log files older than [`LOG_RETENTION_DAYS`]. Entries whose
/// name does not carry a `YYYY-MM-DD` suffix are left untouched. Returns how
/// many files were removed.
pub fn sweep_old_logs() -> usize {
    let Some(dir) = log_dir() else { return 0 };
    sweep_in_dir(&dir, today() - chrono::Duration::days(LOG_RETENTION_DAYS))
}

fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

fn sweep_in_dir(dir: &Path, cutoff: chrono::NaiveDate) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let parsed = name
            .strip_prefix(LOG_PREFIX)
            .and_then(|rest| rest.strip_prefix('.'))
            .and_then(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok());
        if let Some(date) = parsed {
            if date < cutoff && fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

/// Sweep now, then on every calendar-day rollover (checked hourly — an hourly
/// date comparison is far cheaper than an hourly directory scan).
pub fn spawn_retention_sweeper() {
    std::thread::Builder::new()
        .name("log-retention".into())
        .spawn(|| {
            let mut swept_on: Option<chrono::NaiveDate> = None;
            loop {
                let today = today();
                if swept_on != Some(today) {
                    let removed = sweep_old_logs();
                    if removed > 0 {
                        tracing::debug!("log retention: removed {} old file(s)", removed);
                    }
                    swept_on = Some(today);
                }
                std::thread::sleep(Duration::from_secs(60 * 60));
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_fake_log(dir: &Path, date: chrono::NaiveDate) -> PathBuf {
        let path = dir.join(format!("{}.{}", LOG_PREFIX, date.format("%Y-%m-%d")));
        fs::write(&path, b"test").unwrap();
        path
    }

    #[test]
    fn sweep_keeps_recent_and_removes_expired() {
        let dir = std::env::temp_dir().join(format!("netassist-logtest-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let today = today();

        let keep_today = write_fake_log(&dir, today);
        let boundary = write_fake_log(&dir, today - chrono::Duration::days(LOG_RETENTION_DAYS));
        let expired = write_fake_log(&dir, today - chrono::Duration::days(LOG_RETENTION_DAYS + 1));
        // A file that doesn't parse as a dated log file must survive.
        let unrelated = dir.join("anchors.json");
        fs::write(&unrelated, b"x").unwrap();

        let removed = sweep_in_dir(&dir, today - chrono::Duration::days(LOG_RETENTION_DAYS));

        assert_eq!(removed, 1);
        assert!(keep_today.exists(), "today's file must be kept");
        assert!(boundary.exists(), "file exactly at cutoff must be kept");
        assert!(!expired.exists(), "file past cutoff must be removed");
        assert!(unrelated.exists(), "undated files must be untouched");

        fs::remove_dir_all(&dir).ok();
    }
}
