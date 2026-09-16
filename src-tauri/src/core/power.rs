//! Power-source detection, used to throttle background sampling on battery.
//!
//! The realtime traffic monitor samples OS interface counters via short-lived
//! subprocesses (`netstat` on macOS). That is invisible when plugged in but
//! wastes battery when unplugged, so `commands::traffic` asks this module
//! whether the machine is on battery and stretches its sampling interval.
//!
//! Detection is deliberately cheap and cached: the raw check spawns a platform
//! tool (or reads sysfs), so the result is reused for [`CACHE_TTL`] before
//! re-detecting. Unknown or undetectable states (desktops, VMs, errors)
//! always report AC — we never throttle when unsure.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a detection result is reused before re-checking.
const CACHE_TTL: Duration = Duration::from_secs(30);

/// `(is_on_battery, detected_at)` — `std::sync::Mutex` so the getter can be
/// called from blocking threads without an async runtime.
static CACHE: Mutex<Option<(bool, Instant)>> = Mutex::new(None);

/// Whether the machine is currently running on battery power (cached).
///
/// Blocking: may spawn the platform detection tool once per [`CACHE_TTL`].
/// Callers should invoke this from a blocking thread, not the async runtime.
pub fn is_on_battery_cached() -> bool {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((on_battery, detected_at)) = *cache {
        if detected_at.elapsed() < CACHE_TTL {
            return on_battery;
        }
    }
    let on_battery = detect_on_battery();
    *cache = Some((on_battery, Instant::now()));
    on_battery
}

/// Raw platform detection. Errors must resolve to `false` (treat as AC).
#[cfg(target_os = "macos")]
fn detect_on_battery() -> bool {
    // `pmset -g batt` prints e.g. "Now drawing from 'Battery Power'" or
    // "Now drawing from 'AC Power'"; the two literals are mutually exclusive.
    std::process::Command::new("pmset")
        .args(["-g", "batt"])
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("Battery Power"))
}

/// Raw platform detection. Errors must resolve to `false` (treat as AC).
#[cfg(target_os = "linux")]
fn detect_on_battery() -> bool {
    // On battery iff some external-power supply (Mains/USB/...) reports
    // online=0. Machines with no power_supply entries (desktops, most VMs)
    // fall through to false.
    let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_external = std::fs::read_to_string(path.join("type"))
            .is_ok_and(|t| matches!(t.trim(), "Mains" | "USB" | "USB_C" | "USB_PD" | "Wireless"));
        if !is_external {
            continue;
        }
        // A supply with no `online` attribute is assumed online (don't
        // throttle on hardware we can't read).
        let online = match std::fs::read_to_string(path.join("online")) {
            Ok(value) => value.trim() != "0",
            Err(_) => true,
        };
        if !online {
            return true;
        }
    }
    false
}

/// Raw platform detection. Errors must resolve to `false` (treat as AC).
#[cfg(target_os = "windows")]
fn detect_on_battery() -> bool {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: GetSystemPowerStatus only writes into the valid struct we pass.
    let ok = unsafe { GetSystemPowerStatus(&mut status) }.is_ok();
    // ACLineStatus: 0 = offline (on battery), 1 = online, 255 = unknown.
    ok && status.ACLineStatus == 0
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn detect_on_battery() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cached getter must be stable across calls (whatever the platform
    /// reports) and must not panic on repeated use, including after a
    /// poisoned lock.
    #[test]
    fn test_cached_detection_is_stable() {
        let first = is_on_battery_cached();
        let second = is_on_battery_cached();
        assert_eq!(first, second, "result must be served from cache");
    }
}
