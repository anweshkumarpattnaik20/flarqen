#![allow(dead_code)]
use crate::errors::AppError;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn err(code: &str, msg: impl Into<String>, op: &str) -> AppError {
    AppError::new(code, msg, op)
}

fn io(e: std::io::Error, op: &str) -> AppError {
    err("IO_ERROR", format!("File operation failed: {e}"), op)
}

fn os(s: &str) -> OsString {
    OsString::from(s)
}

fn pstr(p: &Path) -> OsString {
    p.as_os_str().to_os_string()
}

/// qemu-img does not reliably accept Windows verbatim paths (\\?\C:\...), so strip the prefix.
pub fn plain_path(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        if !rest.starts_with("UNC\\") {
            return PathBuf::from(rest.to_string());
        }
    }
    p.to_path_buf()
}

/// Absolute paths only: this also makes it impossible for a path to look like a command-line option.
fn require_absolute(p: &Path, op: &str) -> Result<(), AppError> {
    if !p.is_absolute() {
        return Err(err("PATH_NOT_ABSOLUTE", "Disk paths must be absolute", op));
    }
    Ok(())
}

fn canon(p: &Path, op: &str) -> Result<PathBuf, AppError> {
    p.canonicalize().map_err(|e| io(e, op))
}

fn run_qemu_img(qemu_img: &Path, args: &[OsString], op: &str) -> Result<String, AppError> {
    let mut cmd = Command::new(qemu_img);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd
        .output()
        .map_err(|e| err("QEMU_IMG_LAUNCH", format!("Could not run qemu-img: {e}"), op))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(err("QEMU_IMG_FAILED", format!("qemu-img failed: {}", stderr.trim()), op));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[derive(Debug, Deserialize)]
struct RawInfo {
    format: String,
    #[serde(rename = "virtual-size")]
    virtual_size: u64,
    #[serde(rename = "actual-size", default)]
    actual_size: u64,
    #[serde(rename = "backing-filename", default)]
    backing_filename: Option<String>,
    #[serde(rename = "full-backing-filename", default)]
    full_backing_filename: Option<String>,
    #[serde(rename = "backing-filename-format", default)]
    backing_format: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ImageInfo {
    pub format: String,
    pub virtual_size: u64,
    pub actual_size: u64,
    pub backing_filename: Option<String>,
    pub backing_format: Option<String>,
}

pub fn image_info(qemu_img: &Path, image: &Path) -> Result<ImageInfo, AppError> {
    let op = "image_info";
    require_absolute(image, op)?;
    // -U (force share) lets us read info even while a VM has the image open.
    let args = vec![os("info"), os("--output=json"), os("-U"), pstr(image)];
    let out = run_qemu_img(qemu_img, &args, op)?;
    let raw: RawInfo = serde_json::from_str(&out)
        .map_err(|_| err("IMAGE_INFO_PARSE", "Could not read image information", op))?;
    let backing = raw.full_backing_filename.or(raw.backing_filename);
    Ok(ImageInfo {
        format: raw.format,
        virtual_size: raw.virtual_size,
        actual_size: raw.actual_size,
        backing_filename: backing,
        backing_format: raw.backing_format,
    })
}

/// Create an empty QCOW2 master disk (the OS gets installed into it later).
pub fn create_master_image(qemu_img: &Path, path: &Path, size_gb: u64) -> Result<(), AppError> {
    let op = "create_master_image";
    require_absolute(path, op)?;
    if !(1..=2048).contains(&size_gb) {
        return Err(err("SIZE_RANGE", "Disk size must be 1-2048 GB", op));
    }
    if path.exists() {
        return Err(err("IMAGE_EXISTS", "A file already exists at that location", op));
    }
    let args = vec![os("create"), os("-f"), os("qcow2"), pstr(path), os(&format!("{size_gb}G"))];
    run_qemu_img(qemu_img, &args, op)?;
    Ok(())
}

/// Make the master file read-only on disk (a second layer of protection on top of backing-file use).
pub fn protect_master(master: &Path) -> Result<(), AppError> {
    let op = "protect_master";
    let mut perms = fs::metadata(master).map_err(|e| io(e, op))?.permissions();
    perms.set_readonly(true);
    fs::set_permissions(master, perms).map_err(|e| io(e, op))
}

/// Only used by explicit, confirmed image removal and by tests.
pub fn unprotect_master(master: &Path) -> Result<(), AppError> {
    let op = "unprotect_master";
    let mut perms = fs::metadata(master).map_err(|e| io(e, op))?.permissions();
    perms.set_readonly(false);
    fs::set_permissions(master, perms).map_err(|e| io(e, op))
}

/// Confirm the overlay is a qcow2 whose backing file is exactly `expected_master`.
pub fn verify_backing(qemu_img: &Path, overlay: &Path, expected_master: &Path) -> Result<ImageInfo, AppError> {
    let op = "verify_backing";
    let info = image_info(qemu_img, overlay)?;
    if info.format != "qcow2" {
        return Err(err("OVERLAY_FORMAT", "Overlay is not a qcow2 image", op));
    }
    if info.backing_format.as_deref() != Some("qcow2") {
        return Err(err("BACKING_FORMAT", "Backing image format is not qcow2", op));
    }
    let backing = info
        .backing_filename
        .clone()
        .ok_or_else(|| err("NO_BACKING", "Overlay has no backing file", op))?;
    let got = canon(Path::new(&backing), op)?;
    let want = canon(expected_master, op)?;
    if got != want {
        return Err(err("BACKING_MISMATCH", "Overlay points at a different master image", op));
    }
    Ok(info)
}

/// Create a fresh writable overlay on top of `master`, then verify it. Never touches the master.
pub fn create_overlay(qemu_img: &Path, master: &Path, overlay: &Path) -> Result<(), AppError> {
    let op = "create_overlay";
    require_absolute(master, op)?;
    require_absolute(overlay, op)?;
    if !master.is_file() {
        return Err(err("MASTER_MISSING", "Master image does not exist", op));
    }
    if overlay.exists() {
        return Err(err("OVERLAY_EXISTS", "Overlay already exists", op));
    }
    let minfo = image_info(qemu_img, master)?;
    if minfo.format != "qcow2" {
        return Err(err("MASTER_FORMAT", "Master image must be qcow2", op));
    }
    let master_c = plain_path(&canon(master, op)?);
    let args = vec![
        os("create"), os("-f"), os("qcow2"),
        os("-b"), pstr(&master_c),
        os("-F"), os("qcow2"),
        pstr(overlay),
    ];
    run_qemu_img(qemu_img, &args, op)?;
    if let Err(e) = verify_backing(qemu_img, overlay, master) {
        let _ = fs::remove_file(overlay); // remove the bad file we just made
        return Err(e);
    }
    Ok(())
}

/// Transactional reset: build + verify a new overlay beside the old one, then swap.
/// On any failure the original overlay is left (or put back) untouched.
pub fn reset_overlay(qemu_img: &Path, master: &Path, overlay: &Path) -> Result<(), AppError> {
    let op = "reset_overlay";
    require_absolute(master, op)?;
    require_absolute(overlay, op)?;
    if !overlay.is_file() {
        return Err(err("OVERLAY_MISSING", "Sandbox overlay does not exist", op));
    }
    let new = overlay.with_extension("qcow2.new");
    let old = overlay.with_extension("qcow2.old");
    // Leftovers can only come from an earlier interrupted reset; the live overlay exists, so they are stale.
    if new.exists() {
        fs::remove_file(&new).map_err(|e| io(e, op))?;
    }
    if old.exists() {
        fs::remove_file(&old).map_err(|e| io(e, op))?;
    }
    create_overlay(qemu_img, master, &new)?; // also verifies the backing file
    if let Err(e) = fs::rename(overlay, &old) {
        let _ = fs::remove_file(&new);
        return Err(io(e, op));
    }
    if let Err(e) = fs::rename(&new, overlay) {
        let _ = fs::rename(&old, overlay); // roll back
        let _ = fs::remove_file(&new);
        return Err(io(e, op));
    }
    let _ = fs::remove_file(&old);
    verify_backing(qemu_img, overlay, master)?;
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct SelfTestReport {
    pub overlay_virtual_size: u64,
    pub backing_verified: bool,
    pub master_read_only: bool,
}

/// Creates a throwaway master + overlay in `work_dir`, verifies them, and cleans up.
pub fn self_test(qemu_img: &Path, work_dir: &Path) -> Result<SelfTestReport, AppError> {
    let op = "disk_self_test";
    require_absolute(work_dir, op)?;
    fs::create_dir_all(work_dir).map_err(|e| io(e, op))?;
    let master = work_dir.join("master.qcow2");
    let overlay = work_dir.join("overlay.qcow2");

    let run = || -> Result<SelfTestReport, AppError> {
        create_master_image(qemu_img, &master, 1)?;
        protect_master(&master)?;
        create_overlay(qemu_img, &master, &overlay)?;
        let info = verify_backing(qemu_img, &overlay, &master)?;
        let ro = fs::metadata(&master).map(|m| m.permissions().readonly()).unwrap_or(false);
        Ok(SelfTestReport {
            overlay_virtual_size: info.virtual_size,
            backing_verified: true,
            master_read_only: ro,
        })
    };
    let result = run();

    // Cleanup: remove only the files we created, then the (now empty) directory.
    let _ = unprotect_master(&master);
    let _ = fs::remove_file(&master);
    let _ = fs::remove_file(&overlay);
    let _ = fs::remove_dir(work_dir);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    fn qemu_img() -> Option<PathBuf> {
        match crate::qemu::find_qemu_img() {
            Some(t) => Some(PathBuf::from(t.path)),
            None => {
                println!("SKIPPED: qemu-img not found");
                None
            }
        }
    }

    fn qemu_io() -> Option<PathBuf> {
        let p = crate::qemu::find_tool("qemu-io.exe").map(|t| PathBuf::from(t.path));
        if p.is_none() {
            println!("SKIPPED: qemu-io not found");
        }
        p
    }

    fn run_io(io_exe: &Path, image: &Path, cmd: &str) -> String {
        let out = Command::new(io_exe)
            .args(["-f", "qcow2", "-c", cmd])
            .arg(image)
            .output()
            .unwrap();
        assert!(out.status.success(), "qemu-io failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    #[test]
    fn relative_and_option_like_paths_are_rejected() {
        let dummy = Path::new("C:\\does-not-matter\\qemu-img.exe");
        let e = create_overlay(dummy, Path::new("master.qcow2"), Path::new("o.qcow2")).unwrap_err();
        assert_eq!(e.code, "PATH_NOT_ABSOLUTE");
        let e = create_master_image(dummy, Path::new("-evil"), 1).unwrap_err();
        assert_eq!(e.code, "PATH_NOT_ABSOLUTE");
        let e = image_info(dummy, Path::new("--help")).unwrap_err();
        assert_eq!(e.code, "PATH_NOT_ABSOLUTE");
    }

    #[test]
    fn overlay_references_master_and_verifies() {
        let Some(img) = qemu_img() else { return };
        let dir = tempfile::tempdir().unwrap();
        let master = dir.path().join("master.qcow2");
        let overlay = dir.path().join("overlay.qcow2");
        create_master_image(&img, &master, 1).unwrap();
        protect_master(&master).unwrap();
        create_overlay(&img, &master, &overlay).unwrap();
        let info = verify_backing(&img, &overlay, &master).unwrap();
        assert_eq!(info.virtual_size, GB);
        // a different master must NOT verify
        let other = dir.path().join("other.qcow2");
        create_master_image(&img, &other, 1).unwrap();
        let e = verify_backing(&img, &overlay, &other).unwrap_err();
        assert_eq!(e.code, "BACKING_MISMATCH");
        unprotect_master(&master).unwrap();
    }

    #[test]
    fn non_qcow2_master_is_rejected() {
        let Some(img) = qemu_img() else { return };
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("junk.img");
        fs::write(&junk, b"hello").unwrap();
        let overlay = dir.path().join("overlay.qcow2");
        let e = create_overlay(&img, &junk, &overlay).unwrap_err();
        assert_eq!(e.code, "MASTER_FORMAT");
        assert!(!overlay.exists());
    }

    #[test]
    fn guest_writes_never_touch_master() {
        let (Some(img), Some(io_exe)) = (qemu_img(), qemu_io()) else { return };
        let dir = tempfile::tempdir().unwrap();
        let master = dir.path().join("master.qcow2");
        let overlay = dir.path().join("overlay.qcow2");
        create_master_image(&img, &master, 1).unwrap();
        run_io(&io_exe, &master, "write -P 0x11 0 64k"); // put real data in the master
        protect_master(&master).unwrap();
        let before = fs::read(&master).unwrap();

        create_overlay(&img, &master, &overlay).unwrap();
        run_io(&io_exe, &overlay, "write -P 0xab 0 64k"); // "guest" writes into the overlay

        assert_eq!(before, fs::read(&master).unwrap(), "master file changed!");
        let out = run_io(&io_exe, &overlay, "read -P 0xab 0 64k");
        assert!(!out.contains("verification failed"), "overlay lost the write");
        unprotect_master(&master).unwrap();
    }

    #[test]
    fn reset_discards_changes_and_leaves_no_temp_files() {
        let (Some(img), Some(io_exe)) = (qemu_img(), qemu_io()) else { return };
        let dir = tempfile::tempdir().unwrap();
        let master = dir.path().join("master.qcow2");
        let overlay = dir.path().join("overlay.qcow2");
        create_master_image(&img, &master, 1).unwrap();
        protect_master(&master).unwrap();
        create_overlay(&img, &master, &overlay).unwrap();
        run_io(&io_exe, &overlay, "write -P 0xab 0 64k");

        reset_overlay(&img, &master, &overlay).unwrap();

        let out = run_io(&io_exe, &overlay, "read -P 0x00 0 64k"); // baseline is all zeros again
        assert!(!out.contains("verification failed"), "reset did not discard the guest write");
        assert!(!overlay.with_extension("qcow2.new").exists());
        assert!(!overlay.with_extension("qcow2.old").exists());
        verify_backing(&img, &overlay, &master).unwrap();
        unprotect_master(&master).unwrap();
    }

    #[test]
    fn self_test_passes_and_cleans_up() {
        let Some(img) = qemu_img() else { return };
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("st");
        let report = self_test(&img, &work).unwrap();
        assert!(report.backing_verified);
        assert!(report.master_read_only);
        assert_eq!(report.overlay_virtual_size, GB);
        assert!(!work.exists(), "self-test left files behind");
    }
}