use crate::commands::DbState;
use crate::database;
use crate::errors::AppError;
use crate::qemu;
use crate::sandbox_manager::lock;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use tauri::State;

const QEMU_EXE: &str = "qemu-system-x86_64.exe";
const QEMU_IMG_EXE: &str = "qemu-img.exe";

fn err(code: &str, msg: impl Into<String>, op: &str) -> AppError {
    AppError::new(code, msg, op)
}

fn io(e: std::io::Error, op: &str) -> AppError {
    err("IO_ERROR", format!("File operation failed: {e}"), op)
}

fn join_err(e: tauri::Error) -> AppError {
    err("TASK_FAILED", e.to_string(), "task")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Full path to qemu-system-x86_64.exe. `None` means auto-detect.
    pub qemu_path: Option<String>,
    /// Full path to qemu-img.exe. `None` means auto-detect.
    pub qemu_img_path: Option<String>,
    pub default_cpu: u32,
    pub default_ram_mb: i64,
    /// "whpx" or "tcg"
    pub acceleration: String,
    pub log_retention_days: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            qemu_path: None,
            qemu_img_path: None,
            default_cpu: 2,
            default_ram_mb: 2048,
            acceleration: "whpx".to_string(),
            log_retention_days: 90,
        }
    }
}

pub struct SettingsState {
    pub current: Mutex<Settings>,
    pub file: PathBuf,
}

fn clean_opt(v: &Option<String>) -> Option<String> {
    v.as_ref()
        .map(|s| s.trim().trim_matches('"').trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Trims quotes ("Copy as path" adds them) and turns empty strings into `None`.
pub fn normalize(mut s: Settings) -> Settings {
    s.qemu_path = clean_opt(&s.qemu_path);
    s.qemu_img_path = clean_opt(&s.qemu_img_path);
    s.acceleration = s.acceleration.trim().to_ascii_lowercase();
    s
}

/// Checks the path text only (absolute, right file name). Does not touch the disk.
pub fn check_tool_shape(value: &str, expected: &str) -> Result<PathBuf, AppError> {
    let op = "check_tool_shape";
    let p = PathBuf::from(value);
    if !p.is_absolute() {
        return Err(err("PATH_NOT_ABSOLUTE", "Use a full path such as C:\\Program Files\\qemu\\...", op));
    }
    let ok = p
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case(expected))
        .unwrap_or(false);
    if !ok {
        return Err(err("TOOL_NAME", format!("The file must be named {expected}"), op));
    }
    Ok(p)
}

pub fn validate_shape(s: &Settings) -> Result<(), AppError> {
    let op = "validate_settings";
    if !(1..=64).contains(&s.default_cpu) {
        return Err(err("CPU_RANGE", "Default CPU cores must be 1-64", op));
    }
    if !(512..=262_144).contains(&s.default_ram_mb) {
        return Err(err("RAM_RANGE", "Default RAM must be 512-262144 MB", op));
    }
    if s.acceleration != "whpx" && s.acceleration != "tcg" {
        return Err(err("ACCEL_INVALID", "Acceleration must be 'whpx' or 'tcg'", op));
    }
    if !(7..=3650).contains(&s.log_retention_days) {
        return Err(err("RETENTION_RANGE", "Log retention must be 7-3650 days", op));
    }
    if let Some(p) = &s.qemu_path {
        check_tool_shape(p, QEMU_EXE)?;
    }
    if let Some(p) = &s.qemu_img_path {
        check_tool_shape(p, QEMU_IMG_EXE)?;
    }
    Ok(())
}

/// Runs the chosen program with `--version` and checks that it identifies itself as QEMU.
pub fn verify_tool(path: &Path, label: &str) -> Result<(), AppError> {
    let op = "verify_tool";
    if !path.is_file() {
        return Err(err("TOOL_MISSING", format!("{label} was not found at that path"), op));
    }
    let mut cmd = Command::new(path);
    cmd.arg("--version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd
        .output()
        .map_err(|e| err("TOOL_LAUNCH", format!("Could not run {label}: {e}"), op))?;
    let text = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
    if !out.status.success() || !text.contains("qemu") {
        return Err(err("TOOL_INVALID", format!("That file does not identify itself as QEMU ({label})"), op));
    }
    Ok(())
}

fn verify_paths(s: &Settings) -> Result<(), AppError> {
    if let Some(p) = &s.qemu_path {
        verify_tool(Path::new(p), "qemu-system")?;
    }
    if let Some(p) = &s.qemu_img_path {
        verify_tool(Path::new(p), "qemu-img")?;
    }
    Ok(())
}

fn set_aside(file: &Path) {
    let _ = fs::rename(file, file.with_extension("json.bad"));
}

/// Reads the settings file. Missing file gives defaults silently; a corrupt or invalid file is set aside
/// as `settings.json.bad`, defaults are used and a warning text is returned for the activity log.
pub fn load_or_default(file: &Path) -> (Settings, Option<String>) {
    let text = match fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Settings::default(), None),
        Err(e) => {
            return (
                Settings::default(),
                Some(format!("Could not read the settings file ({e}); defaults are in use")),
            )
        }
    };
    match serde_json::from_str::<Settings>(&text) {
        Ok(parsed) => {
            let s = normalize(parsed);
            match validate_shape(&s) {
                Ok(()) => (s, None),
                Err(e) => {
                    set_aside(file);
                    (
                        Settings::default(),
                        Some(format!(
                            "Saved settings were invalid ({}); they were set aside as settings.json.bad and defaults are in use",
                            e.message
                        )),
                    )
                }
            }
        }
        Err(_) => {
            set_aside(file);
            (
                Settings::default(),
                Some("The settings file was corrupt; it was set aside as settings.json.bad and defaults are in use".to_string()),
            )
        }
    }
}

/// Writes to a temporary file first, then renames it over the real one, so a crash cannot leave half a file.
pub fn save_file(file: &Path, s: &Settings) -> Result<(), AppError> {
    let op = "save_settings_file";
    let json = serde_json::to_string_pretty(s).map_err(|e| err("SETTINGS_WRITE", e.to_string(), op))?;
    let tmp = file.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|e| io(e, op))?;
    fs::rename(&tmp, file).map_err(|e| io(e, op))
}

/// Makes the rest of the backend use these settings (currently: the QEMU locations).
pub fn apply(s: &Settings) {
    qemu::set_custom_paths(
        s.qemu_path.as_ref().map(PathBuf::from),
        s.qemu_img_path.as_ref().map(PathBuf::from),
    );
}

/// Deletes activity-log entries older than `days`. Returns how many were removed.
pub fn prune_activity(conn: &Connection, days: u32) -> Result<usize, AppError> {
    let cutoff = database::now() - (days as i64) * 86_400;
    Ok(conn.execute("DELETE FROM activity_log WHERE timestamp < ?1", [cutoff])?)
}

#[tauri::command]
pub fn get_settings(st: State<SettingsState>) -> Result<Settings, AppError> {
    st.current
        .lock()
        .map(|g| g.clone())
        .map_err(|_| err("STATE_LOCK", "Settings are unavailable", "get_settings"))
}

#[tauri::command]
pub async fn save_settings(
    db: State<'_, DbState>,
    st: State<'_, SettingsState>,
    settings: Settings,
) -> Result<Settings, AppError> {
    let op = "save_settings";
    let cleaned = normalize(settings);
    validate_shape(&cleaned)?;

    let for_check = cleaned.clone();
    tauri::async_runtime::spawn_blocking(move || verify_paths(&for_check))
        .await
        .map_err(join_err)??;

    save_file(&st.file, &cleaned)?;
    apply(&cleaned);
    {
        let mut g = st
            .current
            .lock()
            .map_err(|_| err("STATE_LOCK", "Settings are unavailable", op))?;
        *g = cleaned.clone();
    }

    let conn = lock(&db, op)?;
    let pruned = prune_activity(&conn, cleaned.log_retention_days)?;
    database::log_activity(
        &conn,
        "settings_update",
        None,
        "ok",
        &format!("Settings saved ({pruned} old log entries removed)"),
    )?;
    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        validate_shape(&Settings::default()).unwrap();
    }

    #[test]
    fn out_of_range_values_are_rejected() {
        let mut s = Settings::default();
        s.default_cpu = 0;
        assert_eq!(validate_shape(&s).unwrap_err().code, "CPU_RANGE");
        let mut s = Settings::default();
        s.default_ram_mb = 100;
        assert_eq!(validate_shape(&s).unwrap_err().code, "RAM_RANGE");
        let mut s = Settings::default();
        s.acceleration = "kvm".into();
        assert_eq!(validate_shape(&s).unwrap_err().code, "ACCEL_INVALID");
        let mut s = Settings::default();
        s.log_retention_days = 1;
        assert_eq!(validate_shape(&s).unwrap_err().code, "RETENTION_RANGE");
    }

    #[test]
    fn tool_path_shape_checks() {
        assert_eq!(
            check_tool_shape("qemu-img.exe", QEMU_IMG_EXE).unwrap_err().code,
            "PATH_NOT_ABSOLUTE"
        );
        assert_eq!(
            check_tool_shape("C:\\Windows\\notepad.exe", QEMU_IMG_EXE).unwrap_err().code,
            "TOOL_NAME"
        );
        assert!(check_tool_shape("C:\\Tools\\QEMU-IMG.EXE", QEMU_IMG_EXE).is_ok());
        let mut s = Settings::default();
        s.qemu_path = Some("C:\\Windows\\notepad.exe".into());
        assert_eq!(validate_shape(&s).unwrap_err().code, "TOOL_NAME");
    }

    #[test]
    fn normalize_trims_quotes_and_empties() {
        let mut s = Settings::default();
        s.qemu_path = Some("  \"C:\\q\\qemu-system-x86_64.exe\"  ".into());
        s.qemu_img_path = Some("   ".into());
        s.acceleration = " WHPX ".into();
        let n = normalize(s);
        assert_eq!(n.qemu_path.as_deref(), Some("C:\\q\\qemu-system-x86_64.exe"));
        assert_eq!(n.qemu_img_path, None);
        assert_eq!(n.acceleration, "whpx");
    }

    #[test]
    fn save_and_load_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("settings.json");
        let mut s = Settings::default();
        s.default_cpu = 4;
        s.default_ram_mb = 4096;
        s.log_retention_days = 30;
        save_file(&file, &s).unwrap();
        let (loaded, warning) = load_or_default(&file);
        assert_eq!(loaded, s);
        assert!(warning.is_none());
        assert!(!file.with_extension("json.tmp").exists());
    }

    #[test]
    fn missing_file_gives_defaults_without_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let (s, w) = load_or_default(&tmp.path().join("settings.json"));
        assert_eq!(s, Settings::default());
        assert!(w.is_none());
    }

    #[test]
    fn corrupt_file_falls_back_and_is_set_aside() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("settings.json");
        fs::write(&file, "{ this is not json").unwrap();
        let (s, w) = load_or_default(&file);
        assert_eq!(s, Settings::default());
        assert!(w.is_some());
        assert!(!file.exists());
        assert!(file.with_extension("json.bad").exists());
    }

    #[test]
    fn invalid_values_on_disk_fall_back() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("settings.json");
        fs::write(&file, "{\"default_cpu\": 0}").unwrap();
        let (s, w) = load_or_default(&file);
        assert_eq!(s, Settings::default());
        assert!(w.unwrap().contains("invalid"));
        assert!(file.with_extension("json.bad").exists());
    }

    #[test]
    fn prune_removes_only_old_entries() {
        let conn = database::open_in_memory().unwrap();
        let now = database::now();
        conn.execute(
            "INSERT INTO activity_log (operation,sandbox_id,status,message,timestamp) VALUES ('old',NULL,'ok','x',?1)",
            [now - 100 * 86_400],
        )
        .unwrap();
        database::log_activity(&conn, "new", None, "ok", "y").unwrap();
        assert_eq!(prune_activity(&conn, 30).unwrap(), 1);
        let left = database::list_activity(&conn, 10).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].operation, "new");
    }

    #[test]
    fn verifying_a_missing_tool_fails() {
        let e = verify_tool(Path::new("C:\\definitely\\not\\here\\qemu-img.exe"), "qemu-img").unwrap_err();
        assert_eq!(e.code, "TOOL_MISSING");
    }
}