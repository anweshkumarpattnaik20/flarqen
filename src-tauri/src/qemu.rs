use crate::errors::AppError;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Serialize, Clone)]
pub struct ToolInfo {
    pub path: String,
    pub version: String,
}

#[derive(Debug, Serialize)]
pub struct QemuDetection {
    pub qemu: Option<ToolInfo>,
    pub qemu_img: Option<ToolInfo>,
    /// True only if `qemu-system -accel help` lists whpx.
    pub whpx_listed: bool,
}

const QEMU_EXE: &str = "qemu-system-x86_64.exe";
const QEMU_IMG_EXE: &str = "qemu-img.exe";

/// User-chosen locations from the settings file: (qemu-system, qemu-img). `None` means auto-detect.
static CUSTOM: OnceLock<Mutex<(Option<PathBuf>, Option<PathBuf>)>> = OnceLock::new();

fn custom() -> &'static Mutex<(Option<PathBuf>, Option<PathBuf>)> {
    CUSTOM.get_or_init(|| Mutex::new((None, None)))
}

pub fn set_custom_paths(qemu: Option<PathBuf>, qemu_img: Option<PathBuf>) {
    if let Ok(mut g) = custom().lock() {
        *g = (qemu, qemu_img);
    }
}

fn custom_for(exe: &str) -> Option<PathBuf> {
    let g = custom().lock().ok()?;
    if exe.eq_ignore_ascii_case(QEMU_EXE) {
        g.0.clone()
    } else if exe.eq_ignore_ascii_case(QEMU_IMG_EXE) {
        g.1.clone()
    } else {
        None
    }
}

fn run_tool(path: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(path);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

fn candidates(exe: &str) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for var in ["ProgramFiles", "ProgramW6432"] {
        if let Some(base) = std::env::var_os(var) {
            v.push(PathBuf::from(base).join("qemu").join(exe));
        }
    }
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            v.push(dir.join(exe));
        }
    }
    v
}

fn probe_path(p: &Path) -> Option<ToolInfo> {
    if !p.is_file() {
        return None;
    }
    let out = run_tool(p, &["--version"])?;
    Some(ToolInfo {
        path: p.to_string_lossy().to_string(),
        version: first_line(&out),
    })
}

fn probe(exe: &str) -> Option<ToolInfo> {
    // An explicit setting wins. If it is wrong the tool is reported as missing instead of silently
    // falling back to something else, so the user notices and fixes the setting.
    if let Some(p) = custom_for(exe) {
        return probe_path(&p);
    }
    for p in candidates(exe) {
        if let Some(info) = probe_path(&p) {
            return Some(info);
        }
    }
    None
}

pub fn detect() -> QemuDetection {
    let qemu = probe(QEMU_EXE);
    let qemu_img = probe(QEMU_IMG_EXE);
    let whpx_listed = qemu
        .as_ref()
        .and_then(|q| run_tool(Path::new(&q.path), &["-accel", "help"]))
        .map(|o| o.to_lowercase().contains("whpx"))
        .unwrap_or(false);
    QemuDetection { qemu, qemu_img, whpx_listed }
}

pub fn find_qemu_img() -> Option<ToolInfo> {
    probe(QEMU_IMG_EXE)
}

pub fn find_tool(exe: &str) -> Option<ToolInfo> {
    probe(exe)
}

#[tauri::command]
pub async fn detect_qemu() -> Result<QemuDetection, AppError> {
    tauri::async_runtime::spawn_blocking(detect)
        .await
        .map_err(|e| AppError::new("TASK_FAILED", e.to_string(), "detect_qemu"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_takes_only_the_first_line() {
        assert_eq!(first_line("QEMU emulator version 11.1.0\nCopyright"), "QEMU emulator version 11.1.0");
    }

    #[test]
    fn first_line_of_empty_is_empty() {
        assert_eq!(first_line(""), "");
    }

    #[test]
    fn missing_tool_is_none() {
        assert!(probe("definitely-not-a-real-tool.exe").is_none());
    }

    #[test]
    fn bad_custom_qemu_path_is_reported_missing_not_replaced() {
        // Only the qemu-system slot is touched, so tests that use qemu-img are unaffected.
        set_custom_paths(Some(PathBuf::from("C:\\definitely\\not\\here\\qemu-system-x86_64.exe")), None);
        let found = probe(QEMU_EXE);
        set_custom_paths(None, None);
        assert!(found.is_none());
    }
}