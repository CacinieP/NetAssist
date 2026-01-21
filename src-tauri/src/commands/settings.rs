#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;

/// Application settings
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
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
            show_geoip: true,
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
pub enum ValidationError {
    InvalidRefreshInterval(String),
    InvalidDnsServer(String),
    InvalidTrafficLimit(String),
    InvalidLanguage(String),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::InvalidRefreshInterval(msg) => write!(f, "Invalid refresh interval: {}", msg),
            ValidationError::InvalidDnsServer(msg) => write!(f, "Invalid DNS server: {}", msg),
            ValidationError::InvalidTrafficLimit(msg) => write!(f, "Invalid traffic limit: {}", msg),
            ValidationError::InvalidLanguage(msg) => write!(f, "Invalid language: {}", msg),
        }
    }
}

/// Validate DNS server address format
fn validate_dns_server(addr: &str) -> Result<(), ValidationError> {
    if addr.is_empty() {
        return Err(ValidationError::InvalidDnsServer("DNS server cannot be empty".to_string()));
    }

    // Check if it's a valid IP address or hostname
    // Allow IPv4, IPv6, and simple hostnames
    if addr.parse::<std::net::IpAddr>().is_ok() {
        return Ok(());
    }

    // Basic hostname validation (alphanumeric, dots, hyphens)
    if addr.len() > 253 {
        return Err(ValidationError::InvalidDnsServer("DNS server hostname too long".to_string()));
    }

    let hostname_valid = addr.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-');
    if !hostname_valid {
        return Err(ValidationError::InvalidDnsServer(format!("Invalid hostname format: {}", addr)));
    }

    Ok(())
}

/// Validate settings
fn validate_settings(settings: &Settings) -> Result<(), ValidationError> {
    // Validate refresh interval (1-3600 seconds)
    if settings.refresh_interval_secs < 1 || settings.refresh_interval_secs > 3600 {
        return Err(ValidationError::InvalidRefreshInterval(
            format!("must be between 1 and 3600 seconds, got {}", settings.refresh_interval_secs)
        ));
    }

    // Validate DNS servers
    validate_dns_server(&settings.primary_dns)?;
    validate_dns_server(&settings.secondary_dns)?;

    // Validate traffic limit (0.1-10000 GB)
    if settings.traffic_limit_gb < 0.1 || settings.traffic_limit_gb > 10000.0 {
        return Err(ValidationError::InvalidTrafficLimit(
            format!("must be between 0.1 and 10000 GB, got {}", settings.traffic_limit_gb)
        ));
    }

    // Validate language (basic check)
    let valid_languages = ["zh-CN", "en-US", "ja-JP", "ko-KR", "es-ES", "fr-FR", "de-DE", "ru-RU"];
    if !valid_languages.contains(&settings.language.as_str()) {
        return Err(ValidationError::InvalidLanguage(
            format!("unsupported language code: {}", settings.language)
        ));
    }

    Ok(())
}

/// Get settings file path
fn get_settings_path() -> anyhow::Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;

    let app_config_dir = config_dir.join("NetAssist");
    fs::create_dir_all(&app_config_dir)?;

    Ok(app_config_dir.join("settings.json"))
}

/// Load settings from file
fn load_settings_from_file() -> anyhow::Result<Settings> {
    let settings_path = get_settings_path()?;

    if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)?;
        let settings: Settings = serde_json::from_str(&content)?;

        // Validate loaded settings and apply defaults for invalid values
        let mut validated_settings = settings.clone();
        if let Err(e) = validate_settings(&settings) {
            tracing::warn!("Settings validation failed, applying defaults for invalid values: {}", e);

            // Apply defaults for invalid fields
            if settings.refresh_interval_secs < 1 || settings.refresh_interval_secs > 3600 {
                validated_settings.refresh_interval_secs = Settings::default().refresh_interval_secs;
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
            if !["zh-CN", "en-US", "ja-JP", "ko-KR", "es-ES", "fr-FR", "de-DE", "ru-RU"].contains(&settings.language.as_str()) {
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
    fs::write(&settings_path, content)?;
    Ok(())
}

/// Get application settings
#[tauri::command]
pub async fn get_settings() -> Result<Settings, String> {
    match load_settings_from_file() {
        Ok(settings) => Ok(settings),
        Err(e) => {
            tracing::warn!("Failed to load settings, using defaults: {}", e);
            Ok(Settings::default())
        }
    }
}

/// Update application settings
#[tauri::command]
pub async fn update_settings(settings: Settings) -> Result<bool, String> {
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
}

/// Reset settings to default
#[tauri::command]
pub async fn reset_settings() -> Result<Settings, String> {
    let settings = Settings::default();

    if let Err(e) = save_settings_to_file(&settings) {
        let err_msg = format!("Failed to save settings: {}", e);
        tracing::error!("{}", err_msg);
        return Err(err_msg);
    }

    tracing::info!("Settings reset to default");
    Ok(settings)
}

/// Export settings to JSON string
#[tauri::command]
pub async fn export_settings() -> Result<String, String> {
    let settings = load_settings_from_file()
        .map_err(|e| format!("Failed to load settings: {}", e))?;

    serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))
}

/// Import settings from JSON string (with validation)
#[tauri::command]
pub async fn import_settings(json: String) -> Result<Settings, String> {
    // Parse JSON
    let settings: Settings = serde_json::from_str(&json)
        .map_err(|e| format!("Failed to parse settings JSON: {}", e))?;

    // Validate imported settings
    validate_settings(&settings)
        .map_err(|e| format!("Settings validation failed: {}", e))?;

    // Save validated settings
    save_settings_to_file(&settings)
        .map_err(|e| format!("Failed to save settings: {}", e))?;

    tracing::info!("Settings imported successfully");
    Ok(settings)
}

/// Check platform-specific permissions
#[tauri::command]
pub async fn check_platform_permissions() -> Result<serde_json::Value, String> {
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
}

/// Get macOS-specific diagnostics
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn get_macos_diagnostics() -> Result<crate::platform::macos::MacOSDiagnostics, String> {
    crate::platform::run_macos_diagnostics()
        .map_err(|e| e.to_string())
}

/// Get network interface changes
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn get_interface_changes() -> Result<crate::platform::macos::InterfaceChangeEvent, String> {
    crate::platform::detect_interface_changes()
        .map_err(|e| e.to_string())
}
