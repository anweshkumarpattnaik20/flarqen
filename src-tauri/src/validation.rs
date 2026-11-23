#![allow(dead_code)]
use crate::errors::AppError;
use std::path::{Component, Path, PathBuf};

const MAX_NAME_LEN: usize = 40;
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn bad(code: &str, msg: &str, op: &str) -> AppError {
    AppError::new(code, msg, op)
}

/// Sandbox names: 1-40 chars, ASCII letters/digits/space/-/_, no leading or trailing space,
/// not a reserved Windows device name. (On-disk folders use the generated id, never the name.)
pub fn validate_sandbox_name(name: &str) -> Result<(), AppError> {
    let op = "validate_sandbox_name";
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(bad("NAME_LENGTH", "Name must be 1-40 characters", op));
    }
    if name != name.trim() {
        return Err(bad("NAME_WHITESPACE", "Name cannot start or end with a space", op));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_') {
        return Err(bad("NAME_CHARS", "Use only letters, numbers, spaces, - and _", op));
    }
    if RESERVED.contains(&name.to_ascii_uppercase().as_str()) {
        return Err(bad("NAME_RESERVED", "That name is reserved by Windows", op));
    }
    Ok(())
}

pub struct HostLimits {
    pub cpu_threads: u32,
    pub total_ram_mb: i64,
    pub free_disk_gb: i64,
}

// Safety margins (documented in docs/security-model.md later):
// - at most half of the host threads and half of host RAM may go to one sandbox
// - 10 GB of disk always stays free
const DISK_RESERVE_GB: i64 = 10;

pub fn validate_resources(cpu: u32, ram_mb: i64, disk_gb: i64, host: &HostLimits) -> Result<(), AppError> {
    let op = "validate_resources";
    let max_cpu = std::cmp::max(1, host.cpu_threads / 2);
    if cpu < 1 || cpu > max_cpu {
        return Err(bad("CPU_RANGE", &format!("CPU cores must be 1-{max_cpu} on this host"), op));
    }
    let max_ram = host.total_ram_mb / 2;
    if ram_mb < 512 || ram_mb > max_ram {
        return Err(bad("RAM_RANGE", &format!("RAM must be 512-{max_ram} MB on this host"), op));
    }
    let max_disk = std::cmp::min(2048, host.free_disk_gb - DISK_RESERVE_GB);
    if disk_gb < 1 || disk_gb > max_disk {
        return Err(bad("DISK_RANGE", &format!("Disk must be 1-{max_disk} GB (free space minus reserve)"), op));
    }
    Ok(())
}

/// Resolve `candidate` and guarantee the result is strictly inside `base`.
/// Rejects `..`, sibling directories with a shared prefix, and symlinks that escape `base`.
pub fn resolve_inside(base: &Path, candidate: &Path) -> Result<PathBuf, AppError> {
    let op = "resolve_inside";
    let base_c = base
        .canonicalize()
        .map_err(|_| bad("PATH_BASE", "Base directory does not exist", op))?;
    if candidate.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(bad("PATH_TRAVERSAL", "Path may not contain '..'", op));
    }
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base_c.join(candidate)
    };
    let parent = joined.parent().ok_or_else(|| bad("PATH_INVALID", "Invalid path", op))?;
    let name = joined.file_name().ok_or_else(|| bad("PATH_INVALID", "Invalid path", op))?;
    let parent_c = parent
        .canonicalize()
        .map_err(|_| bad("PATH_PARENT", "Parent directory does not exist", op))?;
    let mut resolved = parent_c.join(name);
    if resolved.exists() {
        resolved = resolved
            .canonicalize()
            .map_err(|_| bad("PATH_INVALID", "Cannot resolve path", op))?;
    }
    if resolved == base_c || !resolved.starts_with(&base_c) {
        return Err(bad("PATH_OUTSIDE", "Path is outside the managed directory", op));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> HostLimits {
        HostLimits { cpu_threads: 8, total_ram_mb: 16384, free_disk_gb: 200 }
    }

    #[test]
    fn good_names_pass() {
        assert!(validate_sandbox_name("My Sandbox-1").is_ok());
        assert!(validate_sandbox_name("test_vm").is_ok());
    }

    #[test]
    fn bad_names_fail() {
        for n in ["", " lead", "trail ", "../x", "a/b", "a\\b", "CON", "nul", "name.", "naïve"] {
            assert!(validate_sandbox_name(n).is_err(), "should reject {n:?}");
        }
        assert!(validate_sandbox_name(&"a".repeat(41)).is_err());
    }

    #[test]
    fn default_resources_pass_on_16gb_host() {
        assert!(validate_resources(2, 2048, 20, &host()).is_ok());
    }

    #[test]
    fn over_allocation_is_rejected() {
        assert!(validate_resources(0, 2048, 20, &host()).is_err());
        assert!(validate_resources(5, 2048, 20, &host()).is_err()); // > half of 8 threads
        assert!(validate_resources(2, 256, 20, &host()).is_err());
        assert!(validate_resources(2, 9000, 20, &host()).is_err()); // > half of 16 GB
        assert!(validate_resources(2, 2048, 0, &host()).is_err());
        assert!(validate_resources(2, 2048, 195, &host()).is_err()); // eats the reserve
    }

    #[test]
    fn path_inside_base_is_accepted() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("sandboxes");
        std::fs::create_dir_all(base.join("abc")).unwrap();
        let ok = resolve_inside(&base, Path::new("abc/overlay.qcow2")).unwrap();
        assert!(ok.starts_with(base.canonicalize().unwrap()));
    }

    #[test]
    fn traversal_and_escapes_are_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("sandboxes");
        let sibling = tmp.path().join("sandboxes2"); // shares a name prefix
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        assert!(resolve_inside(&base, Path::new("../sandboxes2/x")).is_err());
        assert!(resolve_inside(&base, &sibling.join("x")).is_err());
        assert!(resolve_inside(&base, Path::new("..")).is_err());
        assert!(resolve_inside(&base, &base).is_err()); // base itself is never a target
    }
}