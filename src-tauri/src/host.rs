use crate::errors::AppError;
use crate::validation::HostLimits;
use std::path::Path;
use sysinfo::{Disks, System};

/// Real host capacity, used to validate resource requests. `for_path` picks which disk to measure.
pub fn host_limits(for_path: &Path) -> Result<HostLimits, AppError> {
    let op = "host_limits";

    let mut sys = System::new()
        .map_err(|e| AppError::new("SYSINFO_FAILED", format!("Could not read host information: {e:?}"), op))?;
    sys.refresh_memory();
    let total_ram_mb = (sys.total_memory() / 1024 / 1024) as i64;
    let cpu_threads = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);

    let disks = Disks::new_with_refreshed_list()
        .map_err(|e| AppError::new("SYSINFO_FAILED", format!("Could not read disk information: {e:?}"), op))?;
    let best = disks
        .iter()
        .filter(|d| for_path.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len());
    let d = best.ok_or_else(|| {
        AppError::new(
            "DISK_UNKNOWN",
            "Could not determine free disk space for the storage location",
            op,
        )
    })?;
    let free_disk_gb = (d.available_space() / 1024 / 1024 / 1024) as i64;

    Ok(HostLimits { cpu_threads, total_ram_mb, free_disk_gb })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_limits_are_sane() {
        let h = host_limits(&std::env::temp_dir()).unwrap();
        assert!(h.cpu_threads >= 1);
        assert!(h.total_ram_mb > 512);
        assert!(h.free_disk_gb >= 0);
    }
}