use crate::commands::DbState;
use crate::database;
use crate::errors::AppError;
use crate::models::SandboxStatus;
use crate::sandbox_manager::{app_data, lock};
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use sysinfo::{Disks, System};
use tauri::State;

/// One long-lived System handle: CPU usage is measured between two refreshes, so it must persist.
pub struct MonitorState(pub Mutex<System>);

const MB: u64 = 1024 * 1024;
const GB: u64 = 1024 * 1024 * 1024;

fn err(code: &str, msg: impl Into<String>, op: &str) -> AppError {
    AppError::new(code, msg, op)
}

#[derive(Debug, Serialize)]
pub struct HostMetrics {
    pub cpu_percent: f32,
    pub cpu_threads: u32,
    pub total_ram_mb: u64,
    pub used_ram_mb: u64,
    pub available_ram_mb: u64,
    pub disk_total_gb: u64,
    pub disk_free_gb: u64,
}

#[derive(Debug, Serialize)]
pub struct StorageInfo {
    pub data_dir: String,
    pub images_dir: String,
    pub sandboxes_dir: String,
    pub images_used_mb: u64,
    pub sandboxes_used_mb: u64,
    pub disk_free_gb: u64,
}

#[derive(Debug, Serialize)]
pub struct SandboxMetrics {
    pub id: String,
    pub name: String,
    pub status: String,
    pub pid: Option<u32>,
    /// Measured: working-set memory of the QEMU process on the host (not the guest's own usage).
    pub memory_mb: Option<u64>,
    /// Measured: size of the overlay file on the host disk.
    pub overlay_mb: u64,
    pub uptime_secs: Option<i64>,
    pub configured_ram_mb: i64,
    pub configured_cpu: u32,
}

/// (total bytes, free bytes) of the disk that contains `path`.
fn disk_space(path: &Path) -> Result<(u64, u64), AppError> {
    let op = "disk_space";
    let disks = Disks::new_with_refreshed_list()
        .map_err(|e| err("SYSINFO_FAILED", format!("Could not read disk information: {e:?}"), op))?;
    let best = disks
        .iter()
        .filter(|d| path.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len());
    let d = best.ok_or_else(|| err("DISK_UNKNOWN", "Could not determine the disk for the storage location", op))?;
    Ok((d.total_space(), d.available_space()))
}

/// Total size of regular files under `path`. Does not follow symlinks; depth-limited.
pub fn dir_size(path: &Path) -> u64 {
    fn walk(p: &Path, depth: u32) -> u64 {
        if depth > 8 {
            return 0;
        }
        let Ok(rd) = fs::read_dir(p) else { return 0 };
        let mut total = 0;
        for e in rd.flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                total += walk(&e.path(), depth + 1);
            } else if m.is_file() {
                total += m.len();
            }
        }
        total
    }
    walk(path, 0)
}

/// Parses one `tasklist /FO CSV /NH` line for a qemu-system process and returns its memory in KB.
/// Works for "50,000 K", "50.000 K" and "50 000 K" style numbers.
pub fn parse_tasklist_mem_kb(line: &str) -> Option<u64> {
    if !line.to_ascii_lowercase().contains("qemu-system") {
        return None;
    }
    let trimmed = line.trim().trim_matches('"');
    let last = trimmed.split("\",\"").last()?;
    let digits: String = last.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn process_mem_mb(pid: u32) -> Option<u64> {
    let mut cmd = Command::new("tasklist");
    cmd.args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().find_map(parse_tasklist_mem_kb).map(|kb| kb / 1024)
}

#[tauri::command]
pub async fn get_host_metrics(
    app: tauri::AppHandle,
    mon: State<'_, MonitorState>,
) -> Result<HostMetrics, AppError> {
    let op = "get_host_metrics";
    let data = app_data(&app, op)?;
    let (disk_total, disk_free) = disk_space(&data)?;
    let mut sys = mon
        .0
        .lock()
        .map_err(|_| err("STATE_LOCK", "Monitor state unavailable", op))?;
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    let threads = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1);
    Ok(HostMetrics {
        cpu_percent: sys.global_cpu_usage(),
        cpu_threads: threads,
        total_ram_mb: sys.total_memory() / MB,
        used_ram_mb: sys.used_memory() / MB,
        available_ram_mb: sys.available_memory() / MB,
        disk_total_gb: disk_total / GB,
        disk_free_gb: disk_free / GB,
    })
}

#[tauri::command]
pub async fn get_storage_info(app: tauri::AppHandle) -> Result<StorageInfo, AppError> {
    let op = "get_storage_info";
    let data = app_data(&app, op)?;
    let images = data.join("images");
    let sandboxes = data.join("sandboxes");
    let (_, free) = disk_space(&data)?;
    let (i2, s2) = (images.clone(), sandboxes.clone());
    let (images_bytes, sandboxes_bytes) =
        tauri::async_runtime::spawn_blocking(move || (dir_size(&i2), dir_size(&s2)))
            .await
            .map_err(|e| err("TASK_FAILED", e.to_string(), op))?;
    Ok(StorageInfo {
        data_dir: data.to_string_lossy().to_string(),
        images_dir: images.to_string_lossy().to_string(),
        sandboxes_dir: sandboxes.to_string_lossy().to_string(),
        images_used_mb: images_bytes / MB,
        sandboxes_used_mb: sandboxes_bytes / MB,
        disk_free_gb: free / GB,
    })
}

#[tauri::command]
pub async fn get_sandbox_metrics(db: State<'_, DbState>) -> Result<Vec<SandboxMetrics>, AppError> {
    let op = "get_sandbox_metrics";
    let list = {
        let conn = lock(&db, op)?;
        database::list_sandboxes(&conn)?
    };
    let now = database::now();
    tauri::async_runtime::spawn_blocking(move || {
        list.into_iter()
            .map(|s| {
                let live = matches!(s.status, SandboxStatus::Running | SandboxStatus::Stopping);
                let memory_mb = if live { s.process_id.and_then(process_mem_mb) } else { None };
                let overlay_mb = fs::metadata(&s.overlay_path).map(|m| m.len() / MB).unwrap_or(0);
                let uptime_secs = if s.status == SandboxStatus::Running {
                    s.last_started_at.map(|t| (now - t).max(0))
                } else {
                    None
                };
                SandboxMetrics {
                    id: s.id,
                    name: s.name,
                    status: s.status.as_str().to_string(),
                    pid: s.process_id,
                    memory_mb,
                    overlay_mb,
                    uptime_secs,
                    configured_ram_mb: s.ram_mb,
                    configured_cpu: s.cpu_cores,
                }
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| err("TASK_FAILED", e.to_string(), op))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_size_sums_nested_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.bin"), vec![0u8; 100]).unwrap();
        fs::create_dir(tmp.path().join("sub")).unwrap();
        fs::write(tmp.path().join("sub").join("b.bin"), vec![0u8; 50]).unwrap();
        assert_eq!(dir_size(tmp.path()), 150);
    }

    #[test]
    fn dir_size_of_missing_dir_is_zero() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(dir_size(&tmp.path().join("nope")), 0);
    }

    #[test]
    fn tasklist_memory_parsing() {
        let line = "\"qemu-system-x86_64.exe\",\"1234\",\"Console\",\"1\",\"50,000 K\"";
        assert_eq!(parse_tasklist_mem_kb(line), Some(50000));
        let dotted = "\"qemu-system-x86_64.exe\",\"1234\",\"Console\",\"1\",\"2.100.500 K\"";
        assert_eq!(parse_tasklist_mem_kb(dotted), Some(2100500));
        assert_eq!(parse_tasklist_mem_kb("\"chrome.exe\",\"1\",\"Console\",\"1\",\"9 K\""), None);
        assert_eq!(parse_tasklist_mem_kb("INFO: No tasks are running"), None);
    }

    #[test]
    fn disk_space_finds_a_disk() {
        let (total, free) = disk_space(&std::env::temp_dir()).unwrap();
        assert!(total > 0);
        assert!(free <= total);
    }
}