//! Traffic history and alert management commands

use crate::models::{
    AlertStatus, CumulativeTraffic, TrafficAlert, TrafficHistory, TrafficHistoryPoint,
};
use chrono::{DateTime, Datelike, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Convert epoch seconds to a local DateTime (epoch → UTC → local).
fn local_from_epoch(secs: i64) -> Option<DateTime<Local>> {
    Utc.timestamp_opt(secs, 0)
        .single()
        .map(|u| u.with_timezone(&Local))
}

/// How many days of per-day history files to keep on disk.
///
/// History and export requests are bounded by this retention window. The
/// 30-day chart fits inside it; live totals use independent counter anchors.
const HISTORY_RETENTION_DAYS: i64 = 45;

/// Traffic history storage state
struct TrafficHistoryStorage {
    data_dir: PathBuf,
    history_cache: HashMap<String, Vec<TrafficHistoryPoint>>,
    /// Local date (`YYYY-MM-DD`) of the last retention sweep, so the sweep
    /// runs at most once per day no matter how often points are recorded.
    last_prune_date: Option<String>,
    /// Persistent totals survive restarts; prior-process counter baselines do not.
    counters_initialized: bool,
    last_history_sample: Option<HistoryCounterSample>,
}

struct HistoryCounterSample {
    snapshot: crate::platform::CounterSnapshot,
    timestamp_ms: i64,
}

impl TrafficHistoryStorage {
    fn new() -> Result<Self, String> {
        let config_dir =
            dirs::config_dir().ok_or_else(|| "Could not find config directory".to_string())?;

        let data_dir = config_dir.join("NetAssist").join("traffic");
        fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        let mut storage = Self {
            data_dir,
            history_cache: HashMap::new(),
            last_prune_date: None,
            counters_initialized: false,
            last_history_sample: None,
        };
        // Sweep on startup so an install that runs for months still prunes
        // even if the day never rolls over while the app is open.
        storage.prune_old_history();
        Ok(storage)
    }

    /// Delete per-day history files older than [`HISTORY_RETENTION_DAYS`].
    ///
    /// Only files named `YYYY-MM-DD.json` (plus stale `.json.tmp` siblings
    /// left by an interrupted atomic write) are considered; anything else —
    /// notably `anchors.json` — is left untouched. Runs at most once per
    /// local date.
    fn prune_old_history(&mut self) {
        let today = Local::now().date_naive();
        let today_str = today.format("%Y-%m-%d").to_string();
        if self.last_prune_date.as_deref() == Some(today_str.as_str()) {
            return;
        }
        self.last_prune_date = Some(today_str);

        let cutoff = today - chrono::Duration::days(HISTORY_RETENTION_DAYS);
        let entries = match fs::read_dir(&self.data_dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!("Retention sweep could not read {:?}: {}", self.data_dir, e);
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            // Accept both "YYYY-MM-DD.json" and stale "YYYY-MM-DD.json.tmp".
            let date_part = match name
                .strip_suffix(".json")
                .or_else(|| name.strip_suffix(".json.tmp"))
            {
                Some(part) => part,
                None => continue,
            };
            // Guards against anything not shaped like a day file (anchors.json,
            // random files the user dropped in the directory, ...).
            if Self::validate_date_format(date_part).is_err() {
                continue;
            }
            let Ok(date) = chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d") else {
                continue;
            };
            if date >= cutoff {
                continue;
            }
            self.history_cache.remove(date_part);
            match fs::remove_file(&path) {
                Ok(()) => tracing::info!("Retention sweep removed {}", name),
                Err(e) => tracing::warn!("Retention sweep failed to remove {}: {}", name, e),
            }
        }
    }

    /// Validate date string format (YYYY-MM-DD) to prevent path traversal
    fn validate_date_format(date_str: &str) -> Result<(), String> {
        // Check length (YYYY-MM-DD = 10 chars)
        if date_str.len() != 10 {
            return Err("Invalid date format: must be YYYY-MM-DD".to_string());
        }

        // Check for NULL bytes and other dangerous characters
        if date_str.contains('\0') || date_str.contains('\n') || date_str.contains('\r') {
            return Err("Invalid date format: contains control characters".to_string());
        }

        // Check for path traversal attempts
        if date_str.contains("..") || date_str.contains('/') || date_str.contains('\\') {
            return Err("Invalid date format: contains path separators".to_string());
        }

        // Validate format using chrono
        use chrono::NaiveDate;
        if NaiveDate::parse_from_str(date_str, "%Y-%m-%d").is_err() {
            return Err("Invalid date format: must be valid YYYY-MM-DD date".to_string());
        }

        // Reasonable date range check (years 2000-2100)
        let year = date_str[0..4]
            .parse::<i32>()
            .map_err(|_| "Invalid year".to_string())?;
        if !(2000..=2100).contains(&year) {
            return Err("Date out of valid range (2000-2100)".to_string());
        }

        Ok(())
    }

    /// Get file path for a specific date (with validation)
    fn get_date_file_path(&self, date_str: &str) -> Result<PathBuf, String> {
        // Validate date string before using it in file path
        Self::validate_date_format(date_str)?;

        Ok(self.data_dir.join(format!("{}.json", date_str)))
    }

    /// Load traffic history for a specific date
    fn load_day_history(&self, date_str: &str) -> Result<Vec<TrafficHistoryPoint>, String> {
        let file_path = self.get_date_file_path(date_str)?;

        if file_path.exists() {
            let content = fs::read_to_string(&file_path)
                .map_err(|e| format!("Failed to read history file: {}", e))?;
            serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse history file: {}", e))
        } else {
            Ok(Vec::new())
        }
    }

    /// Save traffic history for a specific date (atomic: write temp + rename)
    fn save_day_history(&self, date_str: &str, data: &[TrafficHistoryPoint]) -> Result<(), String> {
        let file_path = self.get_date_file_path(date_str)?;
        let content = serde_json::to_string_pretty(data)
            .map_err(|e| format!("Failed to serialize history: {}", e))?;

        let tmp_path = file_path.with_extension("json.tmp");
        fs::write(&tmp_path, content)
            .map_err(|e| format!("Failed to write history file: {}", e))?;
        fs::rename(&tmp_path, &file_path)
            .map_err(|e| format!("Failed to finalize history file: {}", e))?;
        Ok(())
    }

    fn record_counter_read(
        &mut self,
        sample: Result<crate::platform::CounterSnapshot, String>,
        now: DateTime<Local>,
    ) -> Result<(), String> {
        self.record_counter_sample(sample?, now)
    }

    /// Record the actual OS-counter delta over this sampling interval. Rates
    /// supplied by old clients are not integrated: a sampled instantaneous rate
    /// cannot stand in for all traffic during the next five seconds.
    fn record_counter_sample(
        &mut self,
        sample: crate::platform::CounterSnapshot,
        now: DateTime<Local>,
    ) -> Result<(), String> {
        self.cumulative_from_snapshot("day", &sample, now)?;
        let current = HistoryCounterSample {
            snapshot: sample,
            timestamp_ms: now.timestamp_millis(),
        };
        let point = self.last_history_sample.as_ref().and_then(|previous| {
            let elapsed_ms = current.timestamp_ms - previous.timestamp_ms;
            if elapsed_ms <= 0
                || current.snapshot.source != previous.snapshot.source
                || current.snapshot.rx < previous.snapshot.rx
                || current.snapshot.tx < previous.snapshot.tx
            {
                return None;
            }
            let seconds = elapsed_ms as f64 / 1000.0;
            Some(TrafficHistoryPoint {
                interval_start_ms: Some(previous.timestamp_ms),
                timestamp: current.timestamp_ms,
                download_bps: (current.snapshot.rx - previous.snapshot.rx) as f64 / seconds,
                upload_bps: (current.snapshot.tx - previous.snapshot.tx) as f64 / seconds,
            })
        });
        if let Some(point) = point {
            self.add_data_point(point, now)?;
        }
        self.last_history_sample = Some(current);
        Ok(())
    }

    fn add_data_point(
        &mut self,
        point: TrafficHistoryPoint,
        now: DateTime<Local>,
    ) -> Result<(), String> {
        self.prune_old_history();
        let date_str = now.format("%Y-%m-%d").to_string();
        let mut day_data = self.load_day_history(&date_str)?;
        day_data.push(point);
        // A local calendar day may last 25 hours at a DST transition.
        day_data.sort_by_key(|point| point.timestamp);
        self.save_day_history(&date_str, &day_data)?;
        self.history_cache.insert(date_str, day_data);
        Ok(())
    }

    /// Compatibility aggregation for historical files, not a replacement for a
    /// failed live counter read. New points describe a preceding measured
    /// interval. Clipping an interval assumes its measured average rate; it
    /// cannot reconstruct when individual bytes crossed a calendar boundary.
    fn get_cumulative_traffic(&self, period: &str) -> Result<CumulativeTraffic, String> {
        let now = Local::now();
        let (start_time, end_time, label) = Self::period_bounds(period, now)?;
        let start = local_from_epoch(start_time).ok_or("Invalid period start")?;
        // A legacy rate immediately before the period may extend into it.
        let first_file = start.date_naive().pred_opt().unwrap_or(start.date_naive());
        let points = self.load_points_by_date(first_file, now.date_naive())?;
        let (rx, tx) = integrate_history(&points, start_time * 1000, now.timestamp_millis());
        Ok(CumulativeTraffic {
            total_download_bytes: rx,
            total_upload_bytes: tx,
            start_timestamp: start_time,
            end_timestamp: end_time,
            period: label,
        })
    }

    /// Get traffic history for a time range (hours, up to the 45-day retention window)
    fn get_traffic_history(&self, hours: i64) -> Result<TrafficHistory, String> {
        // Honor the 30-day chart option while bounding disk work by retention.
        let hours = hours.clamp(1, 24 * HISTORY_RETENTION_DAYS);
        let now = Local::now();
        let start_time = now - chrono::Duration::hours(hours);

        let data = self.collect_points_between(start_time, now)?;

        Ok(TrafficHistory {
            data,
            start_timestamp: start_time.timestamp_millis(),
            end_timestamp: now.timestamp_millis(),
        })
    }

    /// Get exportable history for a named period: `day` / `week` / `month`
    /// (local calendar bounds, same semantics as the cumulative stats) or
    /// `all` (the whole retention window).
    ///
    /// Exports and charts both honor the full retention window.
    fn get_export_history(&self, period: &str) -> Result<TrafficHistory, String> {
        let now = Local::now();
        let start_time = match period {
            "all" => now - chrono::Duration::days(HISTORY_RETENTION_DAYS),
            "day" | "week" | "month" => {
                use chrono::TimeZone;
                let (start, _, _) = Self::period_bounds(period, now)?;
                Local
                    .timestamp_opt(start, 0)
                    .single()
                    .ok_or_else(|| "Invalid period start".to_string())?
            }
            other => return Err(format!("Unsupported export period: {}", other)),
        };

        let data = self.collect_points_between(start_time, now)?;

        Ok(TrafficHistory {
            data,
            start_timestamp: start_time.timestamp_millis(),
            end_timestamp: now.timestamp_millis(),
        })
    }

    /// Collect every recorded point with
    /// `start.timestamp_millis() <= timestamp <= end.timestamp_millis()`,
    /// walking the per-day files across the range and sorting by timestamp.
    fn collect_points_between(
        &self,
        start: DateTime<Local>,
        end: DateTime<Local>,
    ) -> Result<Vec<TrafficHistoryPoint>, String> {
        let mut all_data = self.load_points_by_date(start.date_naive(), end.date_naive())?;
        all_data.retain(|point| {
            point.timestamp >= start.timestamp_millis() && point.timestamp <= end.timestamp_millis()
        });
        Ok(all_data)
    }

    fn load_points_by_date(
        &self,
        start: chrono::NaiveDate,
        end: chrono::NaiveDate,
    ) -> Result<Vec<TrafficHistoryPoint>, String> {
        let mut all_data = Vec::new();
        for date in calendar_dates(start, end) {
            all_data.extend(self.load_day_history(&date.format("%Y-%m-%d").to_string())?);
        }
        all_data.sort_by_key(|point| point.timestamp);
        Ok(all_data)
    }

    /// Compute the (start, end, label) bounds for a period relative to `now`
    /// in the LOCAL timezone. History files and trend charts label the day by
    /// local date, so boundaries must match the local calendar — with UTC,
    /// "today" for UTC+8 started at 08:00 local and the week started 8h late.
    fn period_bounds(period: &str, now: DateTime<Local>) -> Result<(i64, i64, String), String> {
        use chrono::TimeZone;
        // Some timezone transitions skip midnight or repeat it. Use the
        // earliest valid instant of the calendar date, never the varying `now`
        // as an anchor (that would reset totals on every sample that day).
        let midnight = |date: chrono::NaiveDate| -> Result<DateTime<Local>, String> {
            for minute in 0..(24 * 60) {
                let naive = date.and_hms_opt(minute / 60, minute % 60, 0).unwrap();
                if let Some(instant) = Local.from_local_datetime(&naive).earliest() {
                    return Ok(instant);
                }
            }
            Err(format!(
                "Local calendar date {} has no valid instants",
                date
            ))
        };

        match period {
            "day" => {
                let start = midnight(now.date_naive())?.timestamp();
                Ok((start, now.timestamp(), "day".to_string()))
            }
            "week" => {
                let weekday = now.weekday().num_days_from_monday();
                let monday = now.date_naive() - chrono::Duration::days(weekday as i64);
                let start = midnight(monday)?;
                Ok((start.timestamp(), now.timestamp(), "week".to_string()))
            }
            "month" => {
                let first = now
                    .date_naive()
                    .with_day(1)
                    .ok_or_else(|| "Invalid month".to_string())?;
                let start = midnight(first)?.timestamp();
                Ok((start, now.timestamp(), "month".to_string()))
            }
            _ => Err(format!("Invalid period: {}", period)),
        }
    }

    /// Path to the on-disk anchor store (interface-counter snapshots taken at
    /// the start of each period). Lives next to the per-day history files.
    fn anchors_file_path(&self) -> PathBuf {
        self.data_dir.join("anchors.json")
    }

    /// Only a missing anchor file starts a new store. Read/parse failures are
    /// reported without overwriting the user's persisted totals or evidence.
    fn load_anchors(&self) -> Result<AnchorStore, String> {
        let path = self.anchors_file_path();
        match fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).map_err(|error| {
                format!(
                    "Failed to parse traffic anchors {}: {}",
                    path.display(),
                    error
                )
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(AnchorStore::default())
            }
            Err(error) => Err(format!(
                "Failed to read traffic anchors {}: {}",
                path.display(),
                error
            )),
        }
    }

    fn save_anchors(&self, anchors: &AnchorStore) -> Result<(), String> {
        let path = self.anchors_file_path();
        let content = serde_json::to_string_pretty(anchors).map_err(|error| error.to_string())?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, content)
            .map_err(|error| format!("Failed to write traffic anchors: {}", error))?;
        fs::rename(tmp, path)
            .map_err(|error| format!("Failed to finalize traffic anchors: {}", error))
    }

    fn get_cumulative_traffic_via_counters(
        &mut self,
        period: &str,
    ) -> Result<CumulativeTraffic, String> {
        let sample = crate::platform::get_interface_total_bytes()?;
        self.cumulative_from_snapshot(period, &sample, Local::now())
    }

    /// One successful observation updates every calendar period, independently
    /// of the selected UI tab. Preserve accrued bytes but rebase after process
    /// restart, interface changes or counter resets; offline bytes are unknown.
    fn cumulative_from_snapshot(
        &mut self,
        period: &str,
        sample: &crate::platform::CounterSnapshot,
        now: DateTime<Local>,
    ) -> Result<CumulativeTraffic, String> {
        let (start_ts, end_ts, label) = Self::period_bounds(period, now)?;
        let mut anchors = self.load_anchors()?;
        for (name, anchor) in [
            ("day", &mut anchors.day),
            ("week", &mut anchors.week),
            ("month", &mut anchors.month),
        ] {
            let (start, _, _) = Self::period_bounds(name, now)?;
            if anchor.start_ts != start {
                anchor.start_ts = start;
                anchor.accrued_rx = 0;
                anchor.accrued_tx = 0;
            } else if self.counters_initialized
                && anchor.source == sample.source
                && sample.rx >= anchor.last_rx
                && sample.tx >= anchor.last_tx
            {
                anchor.accrued_rx = anchor.accrued_rx.saturating_add(sample.rx - anchor.last_rx);
                anchor.accrued_tx = anchor.accrued_tx.saturating_add(sample.tx - anchor.last_tx);
            }
            anchor.last_rx = sample.rx;
            anchor.last_tx = sample.tx;
            anchor.source = sample.source.clone();
        }
        self.save_anchors(&anchors)?;
        self.counters_initialized = true;
        let anchor = match period {
            "week" => &anchors.week,
            "month" => &anchors.month,
            _ => &anchors.day,
        };
        Ok(CumulativeTraffic {
            total_download_bytes: anchor.accrued_rx,
            total_upload_bytes: anchor.accrued_tx,
            start_timestamp: start_ts,
            end_timestamp: end_ts,
            period: label,
        })
    }

    #[cfg(test)]
    fn cumulative_from_counters(
        &mut self,
        period: &str,
        rx: u64,
        tx: u64,
    ) -> Result<CumulativeTraffic, String> {
        self.cumulative_from_snapshot(
            period,
            &crate::platform::CounterSnapshot {
                source: "test:en0".into(),
                rx,
                tx,
            },
            Local::now(),
        )
    }
}

/// Iterate calendar dates, never add 24 hours to a zoned datetime (DST can
/// otherwise skip or duplicate a day file).
fn calendar_dates(start: chrono::NaiveDate, end: chrono::NaiveDate) -> Vec<chrono::NaiveDate> {
    let mut dates = Vec::new();
    let mut date = start;
    while date <= end {
        dates.push(date);
        let Some(next) = date.succ_opt() else {
            break;
        };
        date = next;
    }
    dates
}

fn integrate_history(points: &[TrafficHistoryPoint], start_ms: i64, end_ms: i64) -> (u64, u64) {
    let mut points: Vec<_> = points.iter().collect();
    points.sort_by_key(|point| point.timestamp);
    let mut rx = 0.0;
    let mut tx = 0.0;
    for (index, point) in points.iter().enumerate() {
        let (from, to) = if let Some(from) = point.interval_start_ms {
            (from, point.timestamp)
        } else {
            // Legacy samples held until the next global sample. Stop at the
            // start of a measured next interval to avoid mixed-format overlap.
            let next = points
                .get(index + 1)
                .map(|next| next.interval_start_ms.unwrap_or(next.timestamp))
                .unwrap_or(end_ms);
            (
                point.timestamp,
                next.min(point.timestamp.saturating_add(300_000)),
            )
        };
        let duration_ms = to.min(end_ms).saturating_sub(from.max(start_ms)).max(0);
        let seconds = duration_ms as f64 / 1000.0;
        if point.download_bps.is_finite() && point.download_bps >= 0.0 {
            rx += point.download_bps * seconds;
        }
        if point.upload_bps.is_finite() && point.upload_bps >= 0.0 {
            tx += point.upload_bps * seconds;
        }
    }
    (rx.round() as u64, tx.round() as u64)
}

/// Per-period accumulator state.
///
/// - `start_ts` marks which calendar period the accumulator belongs to
///   (rollover detection).
/// - `last_rx/last_tx` are the previously observed OS counters.
/// - `accrued_rx/accrued_tx` is the cumulative traffic for the period so far,
///   which is only ever added to (never reset by counter flapping).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
struct PeriodAnchor {
    #[serde(default)]
    source: String,
    start_ts: i64,
    last_rx: u64,
    last_tx: u64,
    accrued_rx: u64,
    accrued_tx: u64,
}

/// On-disk anchor store: one anchor per supported period. Persisted at
/// `traffic/anchors.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AnchorStore {
    day: PeriodAnchor,
    week: PeriodAnchor,
    month: PeriodAnchor,
}

/// Traffic alert manager
struct TrafficAlertManager {
    alerts_file: PathBuf,
}

impl TrafficAlertManager {
    fn new() -> Result<Self, String> {
        let config_dir =
            dirs::config_dir().ok_or_else(|| "Could not find config directory".to_string())?;

        let data_dir = config_dir.join("NetAssist");
        fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        Ok(Self {
            alerts_file: data_dir.join("alerts.json"),
        })
    }

    /// Load alerts from file
    fn load_alerts(&self) -> Result<Vec<TrafficAlert>, String> {
        if self.alerts_file.exists() {
            let content = fs::read_to_string(&self.alerts_file)
                .map_err(|e| format!("Failed to read alerts file: {}", e))?;
            serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse alerts file: {}", e))
        } else {
            Ok(Self::default_alerts())
        }
    }

    /// Save alerts to file (atomic write)
    fn save_alerts(&self, alerts: &[TrafficAlert]) -> Result<(), String> {
        let content = serde_json::to_string_pretty(alerts)
            .map_err(|e| format!("Failed to serialize alerts: {}", e))?;
        let tmp = self.alerts_file.with_extension("tmp");
        fs::write(&tmp, &content).map_err(|e| format!("Failed to write alerts file: {}", e))?;
        fs::rename(&tmp, &self.alerts_file)
            .map_err(|e| format!("Failed to finalize alerts file: {}", e))?;
        Ok(())
    }

    /// Get default alerts
    fn default_alerts() -> Vec<TrafficAlert> {
        vec![
            TrafficAlert {
                id: "daily_download".to_string(),
                name: "每日下载告警".to_string(),
                alert_type: "download".to_string(),
                threshold_bytes: 50 * 1024 * 1024 * 1024, // 50 GB
                period: "day".to_string(),
                enabled: true,
                triggered: false,
                last_triggered: None,
            },
            TrafficAlert {
                id: "daily_upload".to_string(),
                name: "每日上传告警".to_string(),
                alert_type: "upload".to_string(),
                threshold_bytes: 10 * 1024 * 1024 * 1024, // 10 GB
                period: "day".to_string(),
                enabled: true,
                triggered: false,
                last_triggered: None,
            },
            TrafficAlert {
                id: "monthly_total".to_string(),
                name: "每月总流量告警".to_string(),
                alert_type: "total".to_string(),
                threshold_bytes: 200 * 1024 * 1024 * 1024, // 200 GB
                period: "month".to_string(),
                enabled: true,
                triggered: false,
                last_triggered: None,
            },
        ]
    }

    /// Get all alerts
    fn get_alerts(&self) -> Result<Vec<TrafficAlert>, String> {
        self.load_alerts()
    }

    /// Update an alert
    fn update_alert(&self, alert: TrafficAlert) -> Result<(), String> {
        let mut alerts = self.load_alerts()?;
        let pos = alerts
            .iter()
            .position(|a| a.id == alert.id)
            .ok_or_else(|| format!("Alert not found: {}", alert.id))?;
        alerts[pos] = alert;
        self.save_alerts(&alerts)
    }

    /// Add a new alert
    fn add_alert(&self, alert: TrafficAlert) -> Result<(), String> {
        let mut alerts = self.load_alerts()?;
        // Check if ID already exists
        if alerts.iter().any(|a| a.id == alert.id) {
            return Err(format!("Alert with ID {} already exists", alert.id));
        }
        alerts.push(alert);
        self.save_alerts(&alerts)
    }

    /// Delete an alert
    fn delete_alert(&self, alert_id: &str) -> Result<(), String> {
        let mut alerts = self.load_alerts()?;
        let initial_len = alerts.len();
        alerts.retain(|a| a.id != alert_id);
        if alerts.len() == initial_len {
            return Err(format!("Alert not found: {}", alert_id));
        }
        self.save_alerts(&alerts)
    }

    /// Evaluate every enabled alert against its OWN period using the same
    /// counter-based cumulative method that the UI displays — so the number
    /// shown and the number that triggers an alert always agree. Persists the
    /// `triggered`/`last_triggered` bookkeeping.
    fn check_alerts(
        &self,
        storage: &mut TrafficHistoryStorage,
    ) -> Result<Vec<AlertStatus>, String> {
        let alerts = self.load_alerts()?;
        let mut statuses = Vec::new();
        let mut changed = false;

        for mut alert in alerts {
            if !alert.enabled {
                continue;
            }

            // Evaluate per-alert period (each alert has its own window).
            let cumulative = match storage.get_cumulative_traffic_via_counters(&alert.period) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("alert {} ({}) skipped: {}", alert.name, alert.period, e);
                    continue;
                }
            };

            let current_value = match alert.alert_type.as_str() {
                "download" => cumulative.total_download_bytes,
                "upload" => cumulative.total_upload_bytes,
                "total" => cumulative.total_download_bytes + cumulative.total_upload_bytes,
                _ => continue,
            };

            let triggered = current_value >= alert.threshold_bytes && alert.threshold_bytes > 0;
            let percentage = if alert.threshold_bytes > 0 {
                (current_value as f64 / alert.threshold_bytes as f64) * 100.0
            } else {
                0.0
            };

            // Track trigger state transitions.
            if triggered && !alert.triggered {
                alert.triggered = true;
                alert.last_triggered = Some(Local::now().timestamp_millis());
                changed = true;
            } else if !triggered && alert.triggered {
                alert.triggered = false;
                changed = true;
            }

            statuses.push(AlertStatus {
                alert_id: alert.id.clone(),
                triggered,
                current_value,
                threshold_value: alert.threshold_bytes,
                percentage,
            });

            if changed {
                changed = false;
                let _ = self.update_alert(alert);
            }
        }

        Ok(statuses)
    }
}

// Global instances (using std::sync::OnceLock instead of lazy_static)
use std::sync::OnceLock;

static HISTORY_STORAGE: OnceLock<std::sync::Mutex<TrafficHistoryStorage>> = OnceLock::new();
static ALERT_MANAGER: OnceLock<TrafficAlertManager> = OnceLock::new();

fn history_storage() -> &'static std::sync::Mutex<TrafficHistoryStorage> {
    HISTORY_STORAGE.get_or_init(|| {
        std::sync::Mutex::new(TrafficHistoryStorage::new().unwrap_or_else(|e| {
            tracing::error!("Failed to initialize traffic history storage: {}", e);
            TrafficHistoryStorage {
                data_dir: PathBuf::from("."),
                history_cache: HashMap::new(),
                last_prune_date: None,
                counters_initialized: false,
                last_history_sample: None,
            }
        }))
    })
}

fn alert_manager() -> &'static TrafficAlertManager {
    ALERT_MANAGER.get_or_init(|| {
        TrafficAlertManager::new().unwrap_or_else(|e| {
            tracing::error!("Failed to initialize alert manager: {}", e);
            TrafficAlertManager {
                alerts_file: PathBuf::from("alerts.json"),
            }
        })
    })
}

/// Get cumulative traffic for a time period.
///
/// Uses successful observations of the selected interface's OS counters.
/// A failed read is an error, not a fabricated zero or a rate extrapolation.
/// Totals preserve recorded bytes across restarts but omit unobserved downtime.
#[tauri::command]
pub async fn get_cumulative_traffic(period: String) -> Result<CumulativeTraffic, String> {
    // File I/O inside mutex — run on blocking thread to avoid stalling async runtime
    tokio::task::spawn_blocking(move || {
        let mut storage = history_storage()
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        storage.get_cumulative_traffic_via_counters(&period)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Get traffic history for a time range (hours)
#[tauri::command]
pub async fn get_traffic_history(hours: i64) -> Result<TrafficHistory, String> {
    tokio::task::spawn_blocking(move || {
        let storage = history_storage()
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        storage.get_traffic_history(hours)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Get exportable history for a named period (`day` / `week` / `month` /
/// `all`). Serves the export flow; honors the full retention window instead
/// of the chart-facing 14-day clamp.
#[tauri::command]
pub async fn get_export_traffic_history(period: String) -> Result<TrafficHistory, String> {
    tokio::task::spawn_blocking(move || {
        let storage = history_storage()
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        storage.get_export_history(&period)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Record a traffic data point
#[tauri::command]
pub async fn record_traffic_point(
    download_bps: Option<f64>,
    upload_bps: Option<f64>,
) -> Result<(), String> {
    // Legacy clients may still send these; sampled rates are not byte totals.
    let _ = (download_bps, upload_bps);
    tokio::task::spawn_blocking(move || {
        let mut storage = history_storage()
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        let sample = crate::platform::get_interface_total_bytes();
        storage.record_counter_read(sample, Local::now())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Get all traffic alerts
#[tauri::command]
pub async fn get_traffic_alerts() -> Result<Vec<TrafficAlert>, String> {
    alert_manager().get_alerts()
}

/// Update a traffic alert
#[tauri::command]
pub async fn update_traffic_alert(alert: TrafficAlert) -> Result<(), String> {
    alert_manager().update_alert(alert)
}

/// Add a new traffic alert
#[tauri::command]
pub async fn add_traffic_alert(alert: TrafficAlert) -> Result<(), String> {
    alert_manager().add_alert(alert)
}

/// Delete a traffic alert
#[tauri::command]
pub async fn delete_traffic_alert(alert_id: String) -> Result<(), String> {
    alert_manager().delete_alert(&alert_id)
}

/// Check alert status.
///
/// The `period` argument is accepted for backward compatibility with the old
/// UI, but is intentionally ignored: every alert is evaluated against its own
/// `period` (see `TrafficAlertManager::check_alerts`).
#[tauri::command]
pub async fn check_traffic_alerts(_period: String) -> Result<Vec<AlertStatus>, String> {
    tokio::task::spawn_blocking(move || {
        let mut storage = history_storage()
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        alert_manager().check_alerts(&mut storage)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

    /// Build a `TrafficHistoryStorage` rooted at a fresh temp dir so tests
    /// never touch the real `~/Library/Application Support/NetAssist` data.
    fn storage_in_tempdir() -> (TrafficHistoryStorage, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let data_dir = dir.path().join("traffic");
        fs::create_dir_all(&data_dir).expect("create data_dir");
        let storage = TrafficHistoryStorage {
            data_dir,
            history_cache: HashMap::new(),
            last_prune_date: None,
            counters_initialized: false,
            last_history_sample: None,
        };
        (storage, dir)
    }

    /// `period_bounds("day", ...)` should start at today's 00:00 local and end
    /// at `now`; the same invariant must hold for week (Monday) and month
    /// (1st). This is the shared foundation for both cumulative paths.
    #[test]
    fn test_period_bounds_day() {
        let now = Local::now();
        let (start, end, label) = TrafficHistoryStorage::period_bounds("day", now).unwrap();
        assert_eq!(label, "day");
        assert!(start <= end, "start must not be after end");
        assert_eq!(end, now.timestamp(), "end is the current timestamp");
        let start_dt = local_from_epoch(start).unwrap();
        assert_eq!(start_dt.hour(), 0);
        assert_eq!(start_dt.minute(), 0);
        assert_eq!(start_dt.second(), 0);
    }

    #[test]
    fn test_period_bounds_week_starts_monday() {
        let now = Local::now();
        let (start, _end, _label) = TrafficHistoryStorage::period_bounds("week", now).unwrap();
        let start_dt = local_from_epoch(start).unwrap();
        assert_eq!(
            start_dt.weekday().num_days_from_monday(),
            0,
            "week bound should fall on a Monday"
        );
        assert_eq!(start_dt.hour(), 0);
    }

    #[test]
    fn test_period_bounds_month_starts_first() {
        let now = Local::now();
        let (start, _end, _label) = TrafficHistoryStorage::period_bounds("month", now).unwrap();
        let start_dt = local_from_epoch(start).unwrap();
        assert_eq!(start_dt.day(), 1, "month bound should be the 1st");
        assert_eq!(start_dt.hour(), 0);
    }

    #[test]
    fn test_period_bounds_rejects_unknown_period() {
        let now = Local::now();
        assert!(TrafficHistoryStorage::period_bounds("year", now).is_err());
        assert!(TrafficHistoryStorage::period_bounds("", now).is_err());
    }

    /// First call for a period must create an anchor at the current OS
    /// counters and report 0 bytes consumed (baseline == current).
    #[test]
    fn test_anchor_initial_baselines_to_zero() {
        let (mut storage, _dir) = storage_in_tempdir();

        let result = storage
            .cumulative_from_counters("day", 1_000_000, 2_000_000)
            .expect("counter cumulative succeeds");

        assert_eq!(result.total_download_bytes, 0, "first read is 0");
        assert_eq!(result.total_upload_bytes, 0, "first read is 0");
        assert_eq!(result.period, "day");

        let anchors = storage.load_anchors().unwrap();
        assert_ne!(anchors.day.start_ts, 0, "anchor start_ts was written");
        assert_eq!(anchors.day.last_rx, 1_000_000);
        assert_eq!(anchors.day.last_tx, 2_000_000);
    }

    /// A pre-existing anchor with a stale `start_ts` (period rolled over)
    /// must be re-baselined, so the cumulative total restarts from 0.
    #[test]
    fn test_anchor_period_rollover_rebaselines() {
        let (mut storage, _dir) = storage_in_tempdir();

        let stale = AnchorStore {
            day: PeriodAnchor {
                source: "test:en0".into(),
                start_ts: 946_684_800, // 2000-01-01T00:00:00Z
                last_rx: 0,
                last_tx: 0,
                accrued_rx: 5_000,
                accrued_tx: 5_000,
            },
            ..Default::default()
        };
        storage.save_anchors(&stale).unwrap();

        let result = storage
            .cumulative_from_counters("day", 1_000_000, 1_000_000)
            .expect("counter cumulative succeeds");

        // New period → accumulator reset to 0.
        assert_eq!(result.total_download_bytes, 0);
        assert_eq!(result.total_upload_bytes, 0);

        let anchors = storage.load_anchors().unwrap();
        let (expected_start, _, _) =
            TrafficHistoryStorage::period_bounds("day", Local::now()).unwrap();
        assert_eq!(anchors.day.start_ts, expected_start);
        assert_eq!(anchors.day.accrued_rx, 0);
    }

    /// Monotonic counter growth accrues the delta on every read.
    #[test]
    fn test_anchor_accrues_forward_growth() {
        let (mut storage, _dir) = storage_in_tempdir();

        // First read at 1_000_000 → anchor, 0 accrued.
        let r1 = storage
            .cumulative_from_counters("day", 1_000_000, 2_000_000)
            .unwrap();
        assert_eq!(r1.total_download_bytes, 0);

        // Second read 100 bytes later → accrued 100.
        let r2 = storage
            .cumulative_from_counters("day", 1_000_100, 2_000_500)
            .unwrap();
        assert_eq!(r2.total_download_bytes, 100);
        assert_eq!(r2.total_upload_bytes, 500);

        // Third read another 50 later → accrued 150.
        let r3 = storage
            .cumulative_from_counters("day", 1_000_150, 2_000_500)
            .unwrap();
        assert_eq!(r3.total_download_bytes, 150);
        assert_eq!(r3.total_upload_bytes, 500);
    }

    /// When the OS counters go *backwards* vs the anchor (interface reset /
    /// reboot / Wi-Fi switch / container removal), already-accrued traffic is
    /// preserved — the total must NOT jump to 0, and later growth resumes.
    #[test]
    fn test_anchor_counter_reset_preserves_accrued() {
        let (mut storage, _dir) = storage_in_tempdir();

        let r1 = storage
            .cumulative_from_counters("day", 1_000_000, 1_000_000)
            .unwrap();
        assert_eq!(r1.total_download_bytes, 0);

        // Grow to 1_000_100 (accrued 100).
        storage
            .cumulative_from_counters("day", 1_000_100, 1_000_000)
            .unwrap();

        // Counters reset to a small value (interface flap / reboot).
        let r3 = storage.cumulative_from_counters("day", 100, 50).unwrap();
        assert_eq!(
            r3.total_download_bytes, 100,
            "accrued survives a counter reset"
        );
        assert_eq!(r3.total_upload_bytes, 0);

        // New traffic after reset accrues from the new anchor.
        let r4 = storage.cumulative_from_counters("day", 200, 150).unwrap();
        assert_eq!(r4.total_download_bytes, 200, "100 old + 100 new");
        assert_eq!(r4.total_upload_bytes, 100);
    }

    /// AnchorStore must round-trip through JSON so the persisted baseline
    /// survives app restarts.
    #[test]
    fn test_anchor_store_serde_roundtrip() {
        let store = AnchorStore {
            day: PeriodAnchor {
                source: "test:en0".into(),
                start_ts: 1_700_000_000,
                last_rx: 1234,
                last_tx: 5678,
                accrued_rx: 90,
                accrued_tx: 12,
            },
            week: PeriodAnchor {
                source: "test:en0".into(),
                start_ts: 1_700_000_000,
                last_rx: 9999,
                last_tx: 0,
                accrued_rx: 0,
                accrued_tx: 0,
            },
            month: PeriodAnchor::default(),
        };

        let json = serde_json::to_string(&store).unwrap();
        let back: AnchorStore = serde_json::from_str(&json).unwrap();
        assert_eq!(back.day.last_rx, 1234);
        assert_eq!(back.day.last_tx, 5678);
        assert_eq!(back.day.accrued_rx, 90);
        assert_eq!(back.week.last_rx, 9999);
        assert_eq!(back.month.start_ts, 0);
    }

    #[test]
    fn missing_anchor_file_starts_an_empty_store() {
        let (storage, _dir) = storage_in_tempdir();
        assert_eq!(storage.load_anchors().unwrap().day.start_ts, 0);
        assert!(!storage.anchors_file_path().exists());
    }

    #[test]
    fn corrupt_or_incompatible_anchor_file_is_reported_and_preserved() {
        for original in [
            "not valid json {{{",
            "",
            r#"{"day":{"start_ts":123,"rx_bytes":1,"tx_bytes":2,"carried_rx":3}}"#,
        ] {
            let (mut storage, _dir) = storage_in_tempdir();
            let path = storage.anchors_file_path();
            fs::write(&path, original).unwrap();
            assert!(storage
                .load_anchors()
                .unwrap_err()
                .contains("Failed to parse traffic anchors"));
            assert!(storage.cumulative_from_counters("day", 1000, 1000).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
            assert!(!storage.counters_initialized);
            assert!(!path.with_extension("tmp").exists());
        }
    }

    #[test]
    fn unreadable_anchor_path_is_not_treated_as_a_missing_store() {
        let (mut storage, _dir) = storage_in_tempdir();
        let path = storage.anchors_file_path();
        // A directory produces a deterministic read failure even as root.
        fs::create_dir(&path).unwrap();
        assert!(storage
            .load_anchors()
            .unwrap_err()
            .contains("Failed to read traffic anchors"));
        assert!(storage.cumulative_from_counters("day", 1000, 1000).is_err());
        assert!(path.is_dir());
        assert!(!storage.counters_initialized);
    }

    #[test]
    fn save_failure_preserves_totals_and_does_not_advance_the_baseline() {
        let (mut storage, _dir) = storage_in_tempdir();
        storage.cumulative_from_counters("day", 1000, 1000).unwrap();
        storage.cumulative_from_counters("day", 1100, 1100).unwrap();
        let path = storage.anchors_file_path();
        let original = fs::read(&path).unwrap();
        let tmp = path.with_extension("tmp");
        fs::create_dir(&tmp).unwrap();
        let error = storage
            .cumulative_from_counters("day", 1200, 1200)
            .unwrap_err();
        assert!(error.contains("Failed to write traffic anchors"));
        assert_eq!(fs::read(&path).unwrap(), original);
        fs::remove_dir(tmp).unwrap();
        let resumed = storage.cumulative_from_counters("day", 1300, 1300).unwrap();
        assert_eq!(resumed.total_download_bytes, 300);
        assert_eq!(resumed.total_upload_bytes, 300);
    }

    #[test]
    fn initial_save_failure_does_not_establish_an_unpersisted_baseline() {
        let (mut storage, _dir) = storage_in_tempdir();
        let tmp = storage.anchors_file_path().with_extension("tmp");
        fs::create_dir(&tmp).unwrap();
        assert!(storage.cumulative_from_counters("day", 1000, 1000).is_err());
        assert!(!storage.counters_initialized);
        assert!(!storage.anchors_file_path().exists());
        fs::remove_dir(tmp).unwrap();
        assert_eq!(
            storage
                .cumulative_from_counters("day", 2000, 2000)
                .unwrap()
                .total_download_bytes,
            0
        );
    }

    #[test]
    fn previous_complete_anchor_schema_without_source_remains_readable() {
        let (storage, _dir) = storage_in_tempdir();
        let anchor = serde_json::json!({"start_ts":123,"last_rx":1000,"last_tx":2000,"accrued_rx":50,"accrued_tx":75});
        fs::write(
            storage.anchors_file_path(),
            serde_json::json!({"day":anchor,"week":anchor,"month":anchor}).to_string(),
        )
        .unwrap();
        let store = storage.load_anchors().unwrap();
        assert_eq!(store.day.accrued_rx, 50);
        assert_eq!(store.day.accrued_tx, 75);
        assert!(store.day.source.is_empty());
    }

    /// The retention sweep must delete day files (and stale .json.tmp) older
    /// than the retention window while keeping recent day files and never
    /// touching non-day files like anchors.json.
    #[test]
    fn test_prune_removes_only_files_past_retention() {
        let (mut storage, _dir) = storage_in_tempdir();
        let today = Local::now().date_naive();
        let day = |offset: i64| {
            storage.data_dir.join(format!(
                "{}.json",
                (today - chrono::Duration::days(offset)).format("%Y-%m-%d")
            ))
        };

        let expired = day(HISTORY_RETENTION_DAYS + 1);
        let boundary_keep = day(HISTORY_RETENTION_DAYS - 1);
        let recent = day(3);
        let stale_tmp = expired.with_extension("json.tmp");
        let anchors = storage.anchors_file_path();

        for path in [&expired, &boundary_keep, &recent, &stale_tmp, &anchors] {
            fs::write(path, "[]").unwrap();
        }
        // Seed the cache with the expired date; pruning must evict it.
        let expired_key = (today - chrono::Duration::days(HISTORY_RETENTION_DAYS + 1))
            .format("%Y-%m-%d")
            .to_string();
        storage
            .history_cache
            .insert(expired_key.clone(), Vec::new());

        storage.prune_old_history();
        assert!(
            !storage.history_cache.contains_key(&expired_key),
            "pruned date must be evicted from history_cache"
        );

        assert!(!expired.exists(), "expired day file must be removed");
        assert!(!stale_tmp.exists(), "expired .json.tmp must be removed");
        assert!(boundary_keep.exists(), "file inside window must be kept");
        assert!(recent.exists(), "recent file must be kept");
        assert!(anchors.exists(), "anchors.json must never be pruned");
    }

    /// The sweep is guarded to run at most once per local date; a second call
    /// the same day must be a no-op even if new expired files appear.
    #[test]
    fn test_prune_runs_at_most_once_per_day() {
        let (mut storage, _dir) = storage_in_tempdir();
        storage.prune_old_history();
        assert!(storage.last_prune_date.is_some());

        let today = Local::now().date_naive();
        let expired = storage.data_dir.join(format!(
            "{}.json",
            (today - chrono::Duration::days(HISTORY_RETENTION_DAYS + 5)).format("%Y-%m-%d")
        ));
        fs::write(&expired, "[]").unwrap();

        storage.prune_old_history();
        assert!(
            expired.exists(),
            "second same-day sweep must be skipped by the guard"
        );
    }
    fn snapshot(source: &str, rx: u64, tx: u64) -> crate::platform::CounterSnapshot {
        crate::platform::CounterSnapshot {
            source: source.into(),
            rx,
            tx,
        }
    }

    #[test]
    fn all_periods_accrue_without_being_selected_and_source_switch_rebases() {
        let (mut storage, _dir) = storage_in_tempdir();
        let now = Local::now();
        storage
            .cumulative_from_snapshot("day", &snapshot("en0", 1000, 1000), now)
            .unwrap();
        storage
            .cumulative_from_snapshot("day", &snapshot("en0", 2000, 2000), now)
            .unwrap();
        for period in ["day", "week", "month"] {
            let value = storage
                .cumulative_from_snapshot(period, &snapshot("en0", 2000, 2000), now)
                .unwrap();
            assert_eq!(value.total_download_bytes, 1000);
        }
        let switch = storage
            .cumulative_from_snapshot("month", &snapshot("utun0", 9_000_000, 8_000_000), now)
            .unwrap();
        assert_eq!(switch.total_download_bytes, 1000);
        let growth = storage
            .cumulative_from_snapshot("day", &snapshot("utun0", 9_000_010, 8_000_020), now)
            .unwrap();
        assert_eq!(growth.total_download_bytes, 1010);
        assert_eq!(growth.total_upload_bytes, 1020);
    }

    #[test]
    fn process_restart_preserves_accrued_but_does_not_count_offline_or_reboot_counters() {
        let (mut storage, _dir) = storage_in_tempdir();
        let now = Local::now();
        storage
            .cumulative_from_snapshot("day", &snapshot("en0", 1000, 1000), now)
            .unwrap();
        storage
            .cumulative_from_snapshot("day", &snapshot("en0", 1100, 1100), now)
            .unwrap();
        storage.counters_initialized = false;
        let restarted = storage
            .cumulative_from_snapshot("day", &snapshot("en0", 9_000_000, 9_000_000), now)
            .unwrap();
        assert_eq!(restarted.total_download_bytes, 100);
        let growth = storage
            .cumulative_from_snapshot("day", &snapshot("en0", 9_000_200, 9_000_200), now)
            .unwrap();
        assert_eq!(growth.total_download_bytes, 300);
    }

    #[test]
    fn counter_intervals_preserve_bursts_failures_and_sleep_without_rate_extrapolation() {
        let (mut storage, _dir) = storage_in_tempdir();
        let start = Local::now() - chrono::Duration::hours(2);
        storage
            .record_counter_read(Ok(snapshot("en0", 1_000_000, 100)), start)
            .unwrap();
        assert!(storage
            .record_counter_read(
                Err("OS read failed".into()),
                start + chrono::Duration::seconds(2)
            )
            .is_err());
        // A 1000-byte burst anywhere in the five-second interval is counted once.
        storage
            .record_counter_read(
                Ok(snapshot("en0", 1_001_000, 600)),
                start + chrono::Duration::seconds(5),
            )
            .unwrap();
        // Resume after an hour: only the 200 actually observed bytes are recorded.
        storage
            .record_counter_read(
                Ok(snapshot("en0", 1_001_200, 600)),
                start + chrono::Duration::seconds(3605),
            )
            .unwrap();
        let end = start + chrono::Duration::seconds(4000);
        let points = storage.collect_points_between(start, end).unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].interval_start_ms, Some(start.timestamp_millis()));
        assert_eq!(points[0].download_bps, 200.0);
        assert_eq!(
            integrate_history(&points, start.timestamp_millis(), end.timestamp_millis()),
            (1200, 500)
        );
        // Source change creates a baseline, not a huge history interval.
        storage
            .record_counter_read(Ok(snapshot("utun0", 90_000_000, 80_000_000)), end)
            .unwrap();
        assert_eq!(storage.collect_points_between(start, end).unwrap().len(), 2);
    }

    #[test]
    fn legacy_cross_midnight_and_measured_window_clipping_do_not_overlap() {
        let legacy = vec![
            TrafficHistoryPoint {
                timestamp: 0,
                interval_start_ms: None,
                download_bps: 100.0,
                upload_bps: 0.0,
            },
            TrafficHistoryPoint {
                timestamp: 60_000,
                interval_start_ms: None,
                download_bps: 0.0,
                upload_bps: 0.0,
            },
        ];
        assert_eq!(integrate_history(&legacy, 0, 3_600_000), (6000, 0));
        let measured = vec![TrafficHistoryPoint {
            timestamp: 10_000,
            interval_start_ms: Some(0),
            download_bps: 100.0,
            upload_bps: 10.0,
        }];
        assert_eq!(integrate_history(&measured, 5000, 20_000), (500, 50));
        assert_eq!(integrate_history(&measured, 10_000, 20_000), (0, 0));
        let old: TrafficHistoryPoint =
            serde_json::from_str(r#"{"timestamp":123,"download_bps":1,"upload_bps":2}"#).unwrap();
        assert_eq!(old.interval_start_ms, None);
        let new = serde_json::to_string(&measured[0]).unwrap();
        assert_eq!(
            serde_json::from_str::<TrafficHistoryPoint>(&new)
                .unwrap()
                .interval_start_ms,
            Some(0)
        );
    }

    #[test]
    fn thirty_day_history_is_not_clamped_to_fourteen_days() {
        let (storage, _dir) = storage_in_tempdir();
        let history = storage.get_traffic_history(24 * 30).unwrap();
        assert_eq!(
            history.end_timestamp - history.start_timestamp,
            30 * 24 * 60 * 60 * 1000
        );
        assert_eq!(
            calendar_dates(
                chrono::NaiveDate::from_ymd_opt(2026, 3, 7).unwrap(),
                chrono::NaiveDate::from_ymd_opt(2026, 3, 9).unwrap()
            )
            .len(),
            3
        );
    }

    #[cfg(unix)]
    #[test]
    fn dst_history_includes_every_local_day() {
        const CHILD: &str = "NETASSIST_DST_HISTORY_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "commands::traffic_history::tests::dst_history_includes_every_local_day",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env("TZ", "America/New_York")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        let start = Local
            .with_ymd_and_hms(2026, 3, 7, 23, 30, 0)
            .single()
            .unwrap();
        let end = Local
            .with_ymd_and_hms(2026, 3, 9, 0, 30, 0)
            .single()
            .unwrap();
        assert_eq!((end - start).num_hours(), 24);
        let noon = Local
            .with_ymd_and_hms(2026, 3, 8, 12, 0, 0)
            .single()
            .unwrap();
        let (storage, _dir) = storage_in_tempdir();
        storage
            .save_day_history(
                "2026-03-08",
                &[TrafficHistoryPoint {
                    timestamp: noon.timestamp_millis(),
                    interval_start_ms: None,
                    download_bps: 10.0,
                    upload_bps: 0.0,
                }],
            )
            .unwrap();
        assert_eq!(storage.collect_points_between(start, end).unwrap().len(), 1);
    }
}
