use std::fs;
use std::path::PathBuf;

/// Application settings
///
/// `#[serde(default)]` on the container means a settings.json written by an
/// older version (missing a field added later) still deserializes instead of
/// failing and being silently reset to defaults.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub auto_start: bool,
    pub minimize_to_tray: bool,
    pub refresh_interval_secs: u32,
    pub show_geoip: bool,
    pub primary_dns: String,
    pub secondary_dns: String,
    pub notify_network_abnormal: bool,
    pub notify_traffic_limit: bool,
    pub traffic_limit_gb: f64,
    pub dark_mode: bool,
    pub language: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_start: false,
            minimize_to_tray: true,
            refresh_interval_secs: 1,
            // Off by default: GeoIP resolution goes through third-party
            // online APIs (core/network/geoip.rs), which would send the
            // machine's public IPs to external services without explicit
            // opt-in. Users can enable it in Settings.
            show_geoip: false,
            primary_dns: "8.8.8.8".to_string(),
            secondary_dns: "1.1.1.1".to_string(),
            notify_network_abnormal: true,
            notify_traffic_limit: true,
            traffic_limit_gb: 100.0,
            dark_mode: false,
            language: "zh-CN".to_string(),
        }
    }
}

/// Settings validation error
#[derive(Debug)]
#[allow(clippy::enum_variant_names)] // all variants intentionally share the "Invalid" prefix
pub enum ValidationError {
    InvalidRefreshInterval(String),
    InvalidDnsServer(String),
    InvalidTrafficLimit(String),
    InvalidLanguage(String),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::InvalidRefreshInterval(msg) => {
                write!(f, "Invalid refresh interval: {}", msg)
            }
            ValidationError::InvalidDnsServer(msg) => write!(f, "Invalid DNS server: {}", msg),
            ValidationError::InvalidTrafficLimit(msg) => {
                write!(f, "Invalid traffic limit: {}", msg)
            }
            ValidationError::InvalidLanguage(msg) => write!(f, "Invalid language: {}", msg),
        }
    }
}

/// Validate DNS server address format with comprehensive security checks.
///
/// Only IP literals (IPv4 or IPv6) are accepted: hostnames cannot be used
/// with `networksetup`/`netsh`/`resolvectl` directly nor by the DNS probe
/// (which requires a SocketAddr), so accepting them silently broke those
/// paths later.
fn validate_dns_server(addr: &str) -> Result<(), ValidationError> {
    if addr.is_empty() {
        return Err(ValidationError::InvalidDnsServer(
            "DNS server cannot be empty".to_string(),
        ));
    }

    // Check for NULL bytes and other dangerous characters
    if addr.contains('\0') || addr.contains('\n') || addr.contains('\r') {
        return Err(ValidationError::InvalidDnsServer(
            "DNS server contains invalid characters".to_string(),
        ));
    }

    // Accept bracketed IPv6 ("[::1]") by stripping brackets first.
    let stripped = addr.trim_start_matches('[').trim_end_matches(']');

    // Check if it's a valid IP address (IPv4 or IPv6)
    if stripped.parse::<std::net::IpAddr>().is_ok() {
        return Ok(());
    }

    Err(ValidationError::InvalidDnsServer(format!(
        "must be a valid IPv4 or IPv6 address, got '{}'",
        addr
    )))
}

/// Validate settings
fn validate_settings(settings: &Settings) -> Result<(), ValidationError> {
    // Validate refresh interval (1-3600 seconds)
    if settings.refresh_interval_secs < 1 || settings.refresh_interval_secs > 3600 {
        return Err(ValidationError::InvalidRefreshInterval(format!(
            "must be between 1 and 3600 seconds, got {}",
            settings.refresh_interval_secs
        )));
    }

    // Validate DNS servers
    validate_dns_server(&settings.primary_dns)?;
    validate_dns_server(&settings.secondary_dns)?;

    // Validate traffic limit (0.1-10000 GB)
    if settings.traffic_limit_gb < 0.1 || settings.traffic_limit_gb > 10000.0 {
        return Err(ValidationError::InvalidTrafficLimit(format!(
            "must be between 0.1 and 10000 GB, got {}",
            settings.traffic_limit_gb
        )));
    }

    // Validate language — only zh-CN and en-US are supported (other codes were
    // removed from the dropdown). Previously-stored codes are rejected here and
    // reset to zh-CN at load time.
    let valid_languages = ["zh-CN", "en-US"];
    if !valid_languages.contains(&settings.language.as_str()) {
        return Err(ValidationError::InvalidLanguage(format!(
            "unsupported language code: {}",
            settings.language
        )));
    }

    Ok(())
}

/// Get settings file path
fn get_settings_path() -> anyhow::Result<PathBuf> {
    let config_dir =
        dirs::config_dir().ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;

    let app_config_dir = config_dir.join("NetAssist");
    fs::create_dir_all(&app_config_dir)?;

    Ok(app_config_dir.join("settings.json"))
}

/// Load settings from file
pub fn load_settings_from_file() -> anyhow::Result<Settings> {
    let settings_path = get_settings_path()?;

    if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)?;
        let settings: Settings = serde_json::from_str(&content)?;

        // Validate loaded settings and apply defaults for invalid values
        let mut validated_settings = settings.clone();
        if let Err(e) = validate_settings(&settings) {
            tracing::warn!(
                "Settings validation failed, applying defaults for invalid values: {}",
                e
            );

            // Apply defaults for invalid fields
            if settings.refresh_interval_secs < 1 || settings.refresh_interval_secs > 3600 {
                validated_settings.refresh_interval_secs =
                    Settings::default().refresh_interval_secs;
            }
            if validate_dns_server(&settings.primary_dns).is_err() {
                validated_settings.primary_dns = Settings::default().primary_dns;
            }
            if validate_dns_server(&settings.secondary_dns).is_err() {
                validated_settings.secondary_dns = Settings::default().secondary_dns;
            }
            if settings.traffic_limit_gb < 0.1 || settings.traffic_limit_gb > 10000.0 {
                validated_settings.traffic_limit_gb = Settings::default().traffic_limit_gb;
            }
            if !["zh-CN", "en-US"].contains(&settings.language.as_str()) {
                validated_settings.language = Settings::default().language;
            }
        }

        Ok(validated_settings)
    } else {
        Ok(Settings::default())
    }
}

/// Save settings to file
fn save_settings_to_file(settings: &Settings) -> anyhow::Result<()> {
    // Validate before saving
    validate_settings(settings)
        .map_err(|e| anyhow::anyhow!("Settings validation failed: {}", e))?;

    let settings_path = get_settings_path()?;
    let content = serde_json::to_string_pretty(settings)?;
    // Atomic write: write to a temp file then rename, so an interrupted
    // save can never leave a truncated settings.json behind.
    let tmp_path = settings_path.with_extension("json.tmp");
    fs::write(&tmp_path, content)?;
    fs::rename(&tmp_path, &settings_path)?;
    Ok(())
}

/// Get application settings
#[tauri::command]
pub async fn get_settings() -> Result<Settings, String> {
    tokio::task::spawn_blocking(move || match load_settings_from_file() {
        Ok(settings) => Ok(settings),
        Err(e) => {
            tracing::warn!("Failed to load settings, using defaults: {}", e);
            Ok(Settings::default())
        }
    })
    .await
    .map_err(|e| format!("Settings task join error: {}", e))?
}

/// Update application settings
#[tauri::command]
pub async fn update_settings(settings: Settings) -> Result<bool, String> {
    // Save + validate are file I/O — keep them off the async runtime.
    tokio::task::spawn_blocking(move || {
        // Validate settings before updating
        if let Err(e) = validate_settings(&settings) {
            let err_msg = format!("Settings validation failed: {}", e);
            tracing::error!("{}", err_msg);
            return Err(err_msg);
        }

        // Save to persistent storage
        if let Err(e) = save_settings_to_file(&settings) {
            let err_msg = format!("Failed to save settings: {}", e);
            tracing::error!("{}", err_msg);
            return Err(err_msg);
        }

        // Log without sensitive information
        tracing::info!("Settings updated: auto_start={}, minimize_to_tray={}, refresh_interval_secs={}, dark_mode={}",
            settings.auto_start,
            settings.minimize_to_tray,
            settings.refresh_interval_secs,
            settings.dark_mode
        );
        Ok(true)
    })
    .await
    .map_err(|e| format!("Settings task join error: {}", e))?
}

/// Inject `KeepAlive = { SuccessfulExit = false }` into the LaunchAgent
/// plist after the autostart plugin (re)writes it. The plugin (auto-launch
/// crate) only emits `RunAtLoad`, so without this a crash or kill -9 leaves
/// the monitor dead until next login. `SuccessfulExit: false` relaunches on
/// abnormal exits only — a clean Quit (exit 0) still stays quit. Re-applied
/// on every enable because the plugin overwrites the whole file each time.
#[cfg(target_os = "macos")]
fn inject_launchagent_keepalive() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    // The plugin names the plist after the LaunchAgent label (the product
    // name), not the bundle identifier.
    let plist = std::path::Path::new(&home).join("Library/LaunchAgents/NetAssist.plist");
    if !plist.exists() {
        return;
    }
    match inject_keepalive(&plist) {
        Ok(()) => tracing::info!("LaunchAgent KeepAlive injected ({})", plist.display()),
        Err(e) => tracing::warn!("LaunchAgent KeepAlive injection failed: {}", e),
    }
}

/// plutil-based injection, split out for testability. Remove-then-insert
/// keeps it idempotent (`-insert` fails when the key already exists).
#[cfg(target_os = "macos")]
fn inject_keepalive(plist: &std::path::Path) -> Result<(), String> {
    let run = |args: &[&str]| -> Result<(), String> {
        let out = std::process::Command::new("plutil")
            .args(args)
            .arg(plist)
            .output()
            .map_err(|e| format!("plutil {:?}: {}", args, e))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "plutil {:?}: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            ))
        }
    };
    let _ = run(&["-remove", "KeepAlive"]);
    run(&[
        "-insert",
        "KeepAlive",
        "-json",
        r#"{"SuccessfulExit":false}"#,
    ])
}

/// Enable or disable launch-at-login. Mirrors the settings.auto_start boolean
/// into the OS (LaunchAgent on macOS). The frontend toggles auto_start and
/// then calls this so the change takes effect immediately, not just on save.
#[tauri::command]
pub async fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    match result {
        Ok(_) => {
            // plutil spawns a process — keep it off the runtime's core threads.
            #[cfg(target_os = "macos")]
            if enabled {
                let _ = tokio::task::spawn_blocking(inject_launchagent_keepalive).await;
            }
            tracing::info!("autostart {}", if enabled { "enabled" } else { "disabled" });
            Ok(true)
        }
        Err(e) => {
            let msg = format!(
                "Failed to {} autostart: {}",
                if enabled { "enable" } else { "disable" },
                e
            );
            tracing::error!("{}", msg);
            Err(msg)
        }
    }
}

/// Reset settings to default
#[tauri::command]
pub async fn reset_settings() -> Result<Settings, String> {
    tokio::task::spawn_blocking(|| {
        let settings = Settings::default();

        if let Err(e) = save_settings_to_file(&settings) {
            let err_msg = format!("Failed to save settings: {}", e);
            tracing::error!("{}", err_msg);
            return Err(err_msg);
        }

        tracing::info!("Settings reset to default");
        Ok(settings)
    })
    .await
    .map_err(|e| format!("Settings task join error: {}", e))?
}

/// Check platform-specific permissions.
///
/// The underlying check spawns platform tools (lsof, netstat) — run it on a
/// blocking thread.
#[tauri::command]
pub async fn check_platform_permissions() -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(|| {
        cfg_if::cfg_if! {
            if #[cfg(target_os = "macos")] {
                crate::platform::check_permissions()
                    .map(|status| serde_json::to_value(status).unwrap_or(serde_json::json!({"error": "serialization failed"})))
                    .map_err(|e| e.to_string())
            } else if #[cfg(target_os = "linux")] {
                crate::platform::check_permissions()
                    .map(|status| serde_json::to_value(status).unwrap_or(serde_json::json!({"error": "serialization failed"})))
                    .map_err(|e| e.to_string())
            } else {
                // Windows generally doesn't require special permissions for network monitoring
                Ok(serde_json::json!({
                    "has_permissions": true,
                    "warnings": []
                }))
            }
        }
    })
    .await
    .map_err(|e| format!("Permission task join error: {}", e))?
}

/// Get macOS-specific diagnostics
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn get_macos_diagnostics() -> Result<crate::platform::macos::MacOSDiagnostics, String> {
    crate::platform::run_macos_diagnostics().map_err(|e| e.to_string())
}

/// Get network interface changes
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn get_interface_changes() -> Result<crate::platform::macos::InterfaceChangeEvent, String>
{
    crate::platform::detect_interface_changes().map_err(|e| e.to_string())
}

#[cfg(all(test, target_os = "macos"))]
mod keepalive_tests {
    use super::*;

    /// Injecting into the plugin-shaped plist (RunAtLoad, no KeepAlive) must
    /// add KeepAlive.SuccessfulExit = false, preserve RunAtLoad, and be
    /// idempotent across repeated enables.
    #[test]
    fn test_keepalive_injected_and_idempotent() {
        let dir = std::env::temp_dir().join(format!("netassist-ka-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let plist = dir.join("NetAssist.plist");

        // The exact shape the plugin writes.
        std::fs::write(
            &plist,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
  <key>Label</key>
  <string>NetAssist</string>
  <key>ProgramArguments</key>
  <array><string>/Applications/NetAssist.app/Contents/MacOS/netassist</string></array>
  <key>RunAtLoad</key>
  <true/>
  </dict>
</plist>
"#,
        )
        .unwrap();

        inject_keepalive(&plist).expect("first injection must succeed");
        inject_keepalive(&plist).expect("second injection must succeed (idempotent)");

        let json = std::process::Command::new("plutil")
            .args(["-convert", "json", "-o", "-"])
            .arg(&plist)
            .output()
            .unwrap();
        assert!(json.status.success());
        let stdout = String::from_utf8_lossy(&json.stdout);
        assert!(
            stdout.contains(r#""SuccessfulExit" : false"#)
                || stdout.contains(r#""SuccessfulExit":false"#),
            "KeepAlive.SuccessfulExit must be false, got: {}",
            stdout
        );
        assert!(stdout.contains(r#""RunAtLoad" : true"#) || stdout.contains(r#""RunAtLoad":true"#));

        std::fs::remove_dir_all(&dir).ok();
    }
}
