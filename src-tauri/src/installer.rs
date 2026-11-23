use crate::commands::DbState;
use crate::database;
use crate::disk_manager;
use crate::errors::AppError;
use crate::host;
use crate::models::OsImage;
use crate::validation;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::State;

/// A finished install must use at least this much disk, otherwise the OS was clearly not installed.
const MIN_INSTALLED_BYTES: u64 = 256 * 1024 * 1024;

fn err(code: &str, msg: impl Into<String>, op: &str) -> AppError {
    AppError::new(code, msg, op)
}

fn io(e: std::io::Error, op: &str) -> AppError {
    err("IO_ERROR", format!("File operation failed: {e}"), op)
}

/// Users often paste "Copy as path" output, which has quotes around it.
pub fn clean_user_path(s: &str) -> PathBuf {
    PathBuf::from(s.trim().trim_matches('"'))
}

/// QEMU option values are comma-separated, so a comma in a path would let a path inject extra options.
fn require_option_safe(p: &Path, op: &str) -> Result<(), AppError> {
    let s = p.to_string_lossy();
    if s.contains(',') || s.contains('\n') || s.contains('\r') {
        return Err(err("PATH_UNSAFE", "Paths used by the VM cannot contain commas or line breaks", op));
    }
    Ok(())
}

/// Returns the file size if `path` really is an ISO 9660 image (checks the "CD001" signature).
pub fn validate_iso(path: &Path) -> Result<u64, AppError> {
    let op = "validate_iso";
    if !path.is_absolute() {
        return Err(err("PATH_NOT_ABSOLUTE", "ISO path must be a full path such as C:\\isos\\ubuntu.iso", op));
    }
    let ext_ok = path.extension().map(|e| e.eq_ignore_ascii_case("iso")).unwrap_or(false);
    if !ext_ok {
        return Err(err("ISO_EXTENSION", "The file must have a .iso extension", op));
    }
    require_option_safe(path, op)?;
    let meta = fs::metadata(path).map_err(|_| err("ISO_MISSING", "ISO file not found", op))?;
    if !meta.is_file() {
        return Err(err("ISO_MISSING", "That path is not a file", op));
    }
    let mut f = File::open(path).map_err(|e| io(e, op))?;
    f.seek(SeekFrom::Start(32768)).map_err(|e| io(e, op))?;
    let mut buf = [0u8; 6];
    f.read_exact(&mut buf)
        .map_err(|_| err("ISO_INVALID", "File is too small to be an ISO image", op))?;
    if buf != [1, b'C', b'D', b'0', b'0', b'1'] {
        return Err(err("ISO_INVALID", "File is not a valid ISO 9660 image", op));
    }
    Ok(meta.len())
}

pub fn sha256_file(path: &Path) -> Result<String, AppError> {
    let op = "sha256_file";
    let mut f = File::open(path).map_err(|e| io(e, op))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| io(e, op))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{:02x}", b)).collect::<String>())
}

pub fn normalize_checksum(s: &str) -> Result<String, AppError> {
    let t = s.trim().to_ascii_lowercase();
    if t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(t)
    } else {
        Err(err(
            "CHECKSUM_FORMAT",
            "Checksum must be 64 hexadecimal characters (SHA-256)",
            "normalize_checksum",
        ))
    }
}

pub fn validate_version(v: &str) -> Result<(), AppError> {
    if v.is_empty()
        || v.len() > 20
        || !v.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Err(err(
            "VERSION_INVALID",
            "Version must be 1-20 characters: letters, numbers, . - _",
            "validate_version",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Accel {
    Whpx,
    Tcg,
}

impl Accel {
    pub fn parse(s: Option<&str>) -> Result<Self, AppError> {
        match s.unwrap_or("whpx") {
            "whpx" => Ok(Accel::Whpx),
            "tcg" => Ok(Accel::Tcg),
            _ => Err(err("ACCEL_INVALID", "Acceleration must be 'whpx' or 'tcg'", "parse_accel")),
        }
    }

    fn machine(&self) -> &'static str {
        match self {
            Accel::Whpx => "q35,accel=whpx,kernel-irqchip=off",
            Accel::Tcg => "q35,accel=tcg",
        }
    }
}

/// Fixed argument array. No shell is involved, and the guest gets no network.
pub fn build_installer_args(
    master: &Path,
    iso: &Path,
    cpu: u32,
    ram_mb: i64,
    accel: Accel,
) -> Result<Vec<OsString>, AppError> {
    let op = "build_installer_args";
    require_option_safe(master, op)?;
    require_option_safe(iso, op)?;
    let s = |x: &str| OsString::from(x);
    Ok(vec![
        s("-machine"), s(accel.machine()),
        s("-cpu"), s("qemu64"),
        s("-smp"), s(&cpu.to_string()),
        s("-m"), s(&ram_mb.to_string()),
        s("-name"), s("Disposable OS Sandbox - installer"),
        s("-nic"), s("none"),
        s("-device"), s("qemu-xhci"),
        s("-device"), s("usb-tablet"),
        s("-boot"), s("order=d"),
        s("-drive"), s(&format!("file={},if=virtio,format=qcow2", master.display())),
        s("-drive"), s(&format!("file={},media=cdrom,readonly=on,format=raw", iso.display())),
    ])
}

fn check_image(qemu_img: &Path, image: &Path) -> Result<(), AppError> {
    let op = "check_image";
    let mut cmd = Command::new(qemu_img);
    cmd.arg("check").arg("-f").arg("qcow2").arg(image);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd
        .output()
        .map_err(|e| err("QEMU_IMG_LAUNCH", format!("Could not run qemu-img: {e}"), op))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(err(
            "IMAGE_CORRUPT",
            format!("qemu-img check found problems: {}", String::from_utf8_lossy(&out.stderr).trim()),
            op,
        ))
    }
}

fn read_tail(p: &Path) -> String {
    let b = fs::read(p).unwrap_or_default();
    let start = b.len().saturating_sub(2000);
    String::from_utf8_lossy(&b[start..]).to_string()
}

/// Deletes a directory only if it is strictly inside `images_dir` (never the base itself).
fn remove_managed_dir(images_dir: &Path, dir: &Path) -> Result<(), AppError> {
    let op = "remove_managed_dir";
    let checked = validation::resolve_inside(images_dir, dir)?;
    fs::remove_dir_all(&checked).map_err(|e| io(e, op))
}

// ---------------------------------------------------------------- state + commands

pub struct ActiveInstall {
    child: Child,
    image_id: String,
    name: String,
    version: String,
    dir: PathBuf,
    master: PathBuf,
    log: PathBuf,
}

pub struct InstallState {
    slot: Mutex<Option<ActiveInstall>>,
    busy: AtomicBool,
}

impl InstallState {
    pub fn new() -> Self {
        Self { slot: Mutex::new(None), busy: AtomicBool::new(false) }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartInstallRequest {
    pub name: String,
    pub version: String,
    pub iso_path: String,
    pub disk_gb: u64,
    pub ram_mb: i64,
    pub cpu: u32,
    pub accel: Option<String>,
    pub expected_sha256: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallStarted {
    pub image_id: String,
    pub pid: u32,
    pub master_path: String,
    pub iso_sha256: String,
    pub iso_checksum_verified: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallStatus {
    pub active: bool,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub image_id: Option<String>,
    pub name: Option<String>,
    pub log_tail: Option<String>,
}

fn join_err(e: tauri::Error) -> AppError {
    err("TASK_FAILED", e.to_string(), "task")
}

#[tauri::command]
pub async fn start_master_install(
    app: tauri::AppHandle,
    db: State<'_, DbState>,
    installs: State<'_, InstallState>,
    req: StartInstallRequest,
) -> Result<InstallStarted, AppError> {
    // Claim the single install slot atomically so two clicks can't start two installers.
    if installs
        .busy
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(err(
            "INSTALL_ACTIVE",
            "An installation is already in progress. Finish or cancel it first.",
            "start_master_install",
        ));
    }
    let result = do_start(&app, db.inner(), installs.inner(), req).await;
    if result.is_err() {
        installs.busy.store(false, Ordering::SeqCst);
    }
    result
}

async fn do_start(
    app: &tauri::AppHandle,
    db: &DbState,
    installs: &InstallState,
    req: StartInstallRequest,
) -> Result<InstallStarted, AppError> {
    let op = "start_master_install";
    let qemu = crate::qemu::find_tool("qemu-system-x86_64.exe")
        .ok_or_else(|| err("QEMU_MISSING", "qemu-system-x86_64 was not found", op))?;
    let qemu_img = crate::qemu::find_qemu_img()
        .ok_or_else(|| err("QEMU_IMG_MISSING", "qemu-img was not found", op))?;

    validation::validate_sandbox_name(&req.name)?;
    validate_version(&req.version)?;
    let accel = Accel::parse(req.accel.as_deref())?;
    if req.disk_gb == 0 || req.disk_gb > 2048 {
        return Err(err("DISK_RANGE", "Disk size must be 1-2048 GB", op));
    }

    let data = crate::sandbox_manager::app_data(app, op)?;
    let images_dir = data.join("images");
    fs::create_dir_all(&images_dir).map_err(|e| io(e, op))?;

    let host = host::host_limits(&images_dir)?;
    validation::validate_resources(req.cpu, req.ram_mb, req.disk_gb as i64, &host)?;

    let iso = clean_user_path(&req.iso_path);
    let iso_size = validate_iso(&iso)?;
    let expected = match req.expected_sha256.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Some(normalize_checksum(s)?),
        None => None,
    };

    let iso_for_hash = iso.clone();
    let sha = tauri::async_runtime::spawn_blocking(move || sha256_file(&iso_for_hash))
        .await
        .map_err(join_err)??;
    let iso_verified = match &expected {
        Some(e) if *e != sha => {
            return Err(err(
                "CHECKSUM_MISMATCH",
                "The ISO's SHA-256 does not match the checksum you entered. Do not use this file.",
                op,
            ));
        }
        Some(_) => true,
        None => false,
    };

    let image_id = uuid::Uuid::new_v4().to_string();
    let dir_raw = images_dir.join(&image_id);
    fs::create_dir(&dir_raw).map_err(|e| io(e, op))?;
    let dir = disk_manager::plain_path(&validation::resolve_inside(&images_dir, &dir_raw)?);
    let master = dir.join("master.qcow2");
    let log = dir.join("installer.log");

    let cleanup = |dir: &Path| {
        let _ = remove_managed_dir(&images_dir, dir);
    };

    if let Err(e) = disk_manager::create_master_image(Path::new(&qemu_img.path), &master, req.disk_gb) {
        cleanup(&dir);
        return Err(e);
    }
    let args = match build_installer_args(&master, &iso, req.cpu, req.ram_mb, accel) {
        Ok(a) => a,
        Err(e) => {
            cleanup(&dir);
            return Err(e);
        }
    };
    let (log_out, log_err) = match File::create(&log).and_then(|f| f.try_clone().map(|g| (f, g))) {
        Ok(p) => p,
        Err(e) => {
            cleanup(&dir);
            return Err(io(e, op));
        }
    };

    let mut cmd = Command::new(&qemu.path);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_out))
        .stderr(Stdio::from(log_err));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            cleanup(&dir);
            return Err(err("QEMU_LAUNCH", format!("Could not start QEMU: {e}"), op));
        }
    };

    // If QEMU is going to fail on startup (bad acceleration, bad args), it does so within a couple of seconds.
    let (child, status) = tauri::async_runtime::spawn_blocking(move || {
        let mut child = child;
        std::thread::sleep(std::time::Duration::from_millis(2500));
        let st = child.try_wait();
        (child, st)
    })
    .await
    .map_err(join_err)?;
    if let Ok(Some(st)) = status {
        let tail = read_tail(&log);
        cleanup(&dir);
        return Err(err(
            "QEMU_EXITED_EARLY",
            format!("QEMU exited immediately ({st}). Output: {}", tail.trim()),
            op,
        ));
    }

    let pid = child.id();
    {
        let mut g = installs
            .slot
            .lock()
            .map_err(|_| err("STATE_LOCK", "Install state unavailable", op))?;
        *g = Some(ActiveInstall {
            child,
            image_id: image_id.clone(),
            name: req.name.clone(),
            version: req.version.clone(),
            dir,
            master: master.clone(),
            log,
        });
    }
    if let Ok(conn) = db.0.lock() {
        let _ = database::log_activity(
            &conn,
            "master_install_start",
            None,
            "ok",
            &format!(
                "Installer started for '{}' (ISO {} MB, sha256 {}, {})",
                req.name,
                iso_size / 1024 / 1024,
                sha,
                if iso_verified { "matches the checksum entered" } else { "NOT checked against a published checksum" }
            ),
        );
    }
    Ok(InstallStarted {
        image_id,
        pid,
        master_path: master.to_string_lossy().to_string(),
        iso_sha256: sha,
        iso_checksum_verified: iso_verified,
    })
}

#[tauri::command]
pub fn get_install_status(installs: State<InstallState>) -> Result<InstallStatus, AppError> {
    let op = "get_install_status";
    let mut g = installs
        .slot
        .lock()
        .map_err(|_| err("STATE_LOCK", "Install state unavailable", op))?;
    match g.as_mut() {
        None => Ok(InstallStatus {
            active: false,
            running: false,
            exit_code: None,
            image_id: None,
            name: None,
            log_tail: None,
        }),
        Some(a) => {
            let st = a.child.try_wait().map_err(|e| io(e, op))?;
            Ok(InstallStatus {
                active: true,
                running: st.is_none(),
                exit_code: st.and_then(|s| s.code()),
                image_id: Some(a.image_id.clone()),
                name: Some(a.name.clone()),
                log_tail: Some(read_tail(&a.log)),
            })
        }
    }
}

#[tauri::command]
pub async fn finish_master_install(
    db: State<'_, DbState>,
    installs: State<'_, InstallState>,
) -> Result<OsImage, AppError> {
    let op = "finish_master_install";
    let (image_id, name, version, master) = {
        let mut g = installs
            .slot
            .lock()
            .map_err(|_| err("STATE_LOCK", "Install state unavailable", op))?;
        let a = g
            .as_mut()
            .ok_or_else(|| err("NO_INSTALL", "No installation is in progress", op))?;
        match a.child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => {
                return Err(err(
                    "INSTALL_STILL_RUNNING",
                    "The installer VM is still running. Shut it down (close its window) first.",
                    op,
                ))
            }
            Err(e) => return Err(io(e, op)),
        }
        (a.image_id.clone(), a.name.clone(), a.version.clone(), a.master.clone())
    };

    let qemu_img = crate::qemu::find_qemu_img()
        .ok_or_else(|| err("QEMU_IMG_MISSING", "qemu-img was not found", op))?;
    let m2 = master.clone();
    let (checksum, actual) = tauri::async_runtime::spawn_blocking(move || -> Result<(String, u64), AppError> {
        let qi = PathBuf::from(&qemu_img.path);
        let info = disk_manager::image_info(&qi, &m2)?;
        if info.format != "qcow2" {
            return Err(err("MASTER_FORMAT", "Master disk is not qcow2", op));
        }
        if info.actual_size < MIN_INSTALLED_BYTES {
            return Err(err(
                "INSTALL_INCOMPLETE",
                "The disk looks unused, so the OS install did not complete. Cancel and try again.",
                op,
            ));
        }
        check_image(&qi, &m2)?;
        let sum = sha256_file(&m2)?;
        disk_manager::protect_master(&m2)?;
        Ok((sum, info.actual_size))
    })
    .await
    .map_err(join_err)??;

    let img = OsImage {
        id: image_id,
        name,
        version,
        image_path: master.to_string_lossy().to_string(),
        image_format: "qcow2".into(),
        checksum: Some(checksum),
        verified: true,
        created_at: database::now(),
    };
    {
        let conn = db.0.lock().map_err(|_| err("DB_LOCK", "Database is unavailable", op))?;
        database::insert_image(&conn, &img)?;
        let _ = database::log_activity(
            &conn,
            "master_install_finish",
            None,
            "ok",
            &format!("Master image '{}' registered ({} MB on disk)", img.name, actual / 1024 / 1024),
        );
    }
    {
        let mut g = installs
            .slot
            .lock()
            .map_err(|_| err("STATE_LOCK", "Install state unavailable", op))?;
        *g = None;
    }
    installs.busy.store(false, Ordering::SeqCst);
    Ok(img)
}

#[tauri::command]
pub async fn cancel_master_install(
    app: tauri::AppHandle,
    db: State<'_, DbState>,
    installs: State<'_, InstallState>,
) -> Result<(), AppError> {
    let op = "cancel_master_install";
    let active = {
        let mut g = installs
            .slot
            .lock()
            .map_err(|_| err("STATE_LOCK", "Install state unavailable", op))?;
        g.take()
    };
    let Some(mut a) = active else {
        return Err(err("NO_INSTALL", "No installation is in progress", op));
    };
    installs.busy.store(false, Ordering::SeqCst);
    let images_dir = crate::sandbox_manager::app_data(&app, op)?.join("images");
    let name = a.name.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<(), AppError> {
        let _ = a.child.kill();
        let _ = a.child.wait();
        remove_managed_dir(&images_dir, &a.dir)
    })
    .await
    .map_err(join_err)?;
    if let Ok(conn) = db.0.lock() {
        let (st, msg) = match &result {
            Ok(_) => ("ok".to_string(), format!("Installation of '{name}' cancelled and files removed")),
            Err(e) => ("error".to_string(), e.message.clone()),
        };
        let _ = database::log_activity(&conn, "master_install_cancel", None, &st, &msg);
    }
    result
}

#[tauri::command]
pub fn list_os_images(db: State<DbState>) -> Result<Vec<OsImage>, AppError> {
    let conn = db
        .0
        .lock()
        .map_err(|_| err("DB_LOCK", "Database is unavailable", "list_os_images"))?;
    let mut stmt = conn.prepare(
        "SELECT id,name,version,image_path,image_format,checksum,verified,created_at
         FROM os_images ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(OsImage {
            id: r.get(0)?,
            name: r.get(1)?,
            version: r.get(2)?,
            image_path: r.get(3)?,
            image_format: r.get(4)?,
            checksum: r.get(5)?,
            verified: r.get::<_, i64>(6)? != 0,
            created_at: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_iso(dir: &Path, name: &str, sig_ok: bool) -> PathBuf {
        let p = dir.join(name);
        let mut data = vec![0u8; 40000];
        if sig_ok {
            data[32768] = 1;
            data[32769..32774].copy_from_slice(b"CD001");
        }
        fs::write(&p, data).unwrap();
        p
    }

    #[test]
    fn iso_validation_accepts_real_signature() {
        let tmp = tempfile::tempdir().unwrap();
        let p = write_iso(tmp.path(), "ok.iso", true);
        assert_eq!(validate_iso(&p).unwrap(), 40000);
    }

    #[test]
    fn iso_validation_rejects_bad_files() {
        let tmp = tempfile::tempdir().unwrap();
        let bad_sig = write_iso(tmp.path(), "bad.iso", false);
        assert_eq!(validate_iso(&bad_sig).unwrap_err().code, "ISO_INVALID");
        let small = tmp.path().join("small.iso");
        fs::write(&small, b"hi").unwrap();
        assert_eq!(validate_iso(&small).unwrap_err().code, "ISO_INVALID");
        let wrong_ext = write_iso(tmp.path(), "x.img", true);
        assert_eq!(validate_iso(&wrong_ext).unwrap_err().code, "ISO_EXTENSION");
        assert_eq!(validate_iso(Path::new("ubuntu.iso")).unwrap_err().code, "PATH_NOT_ABSOLUTE");
        assert_eq!(validate_iso(&tmp.path().join("a,b.iso")).unwrap_err().code, "PATH_UNSAFE");
        assert_eq!(validate_iso(&tmp.path().join("nope.iso")).unwrap_err().code, "ISO_MISSING");
    }

    #[test]
    fn sha256_matches_known_vector() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("abc.txt");
        fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_file(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn checksum_normalization() {
        let good = "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD";
        assert_eq!(normalize_checksum(&format!("  {good}  ")).unwrap(), good.to_ascii_lowercase());
        assert!(normalize_checksum("abc").is_err());
        assert!(normalize_checksum(&"z".repeat(64)).is_err());
    }

    #[test]
    fn version_validation() {
        assert!(validate_version("24.04").is_ok());
        assert!(validate_version("").is_err());
        assert!(validate_version("a b").is_err());
        assert!(validate_version("../x").is_err());
        assert!(validate_version(&"1".repeat(21)).is_err());
    }

    #[test]
    fn accel_parsing() {
        assert_eq!(Accel::parse(None).unwrap(), Accel::Whpx);
        assert_eq!(Accel::parse(Some("tcg")).unwrap(), Accel::Tcg);
        assert!(Accel::parse(Some("kvm")).is_err());
    }

    #[test]
    fn installer_args_are_isolated_and_safe() {
        let m = Path::new("C:\\vm\\master.qcow2");
        let i = Path::new("C:\\iso\\u.iso");
        let a: Vec<String> = build_installer_args(m, i, 2, 4096, Accel::Whpx)
            .unwrap()
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        let pos = a.iter().position(|x| x == "-nic").unwrap();
        assert_eq!(a[pos + 1], "none"); // no guest network
        assert!(a.iter().any(|x| x.contains("accel=whpx")));
        assert!(!a.iter().any(|x| x == "-netdev" || x == "-virtfs" || x == "-usb"));
        assert!(a.iter().any(|x| x.starts_with("file=C:\\vm\\master.qcow2,")));
        let bad = build_installer_args(Path::new("C:\\a,b\\m.qcow2"), i, 2, 4096, Accel::Whpx);
        assert_eq!(bad.unwrap_err().code, "PATH_UNSAFE");
    }

    #[test]
    fn managed_dir_removal_is_confined() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("images");
        let inside = base.join("abc");
        let sibling = tmp.path().join("images2");
        fs::create_dir_all(&inside).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        fs::write(inside.join("f.txt"), b"x").unwrap();
        assert!(remove_managed_dir(&base, &base).is_err());
        assert!(remove_managed_dir(&base, &sibling).is_err());
        assert!(sibling.exists() && base.exists());
        remove_managed_dir(&base, &inside).unwrap();
        assert!(!inside.exists());
    }

    #[test]
    fn read_tail_returns_last_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("l.log");
        fs::write(&p, format!("{}END", "a".repeat(5000))).unwrap();
        let t = read_tail(&p);
        assert!(t.len() <= 2000);
        assert!(t.ends_with("END"));
    }
}