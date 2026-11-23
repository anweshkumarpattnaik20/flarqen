use crate::commands::DbState;
use crate::database;
use crate::disk_manager;
use crate::errors::AppError;
use crate::host;
use crate::installer;
use crate::models::{OsImage, Sandbox, SandboxStatus};
use crate::validation;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{Manager, State};

const GIB: u64 = 1024 * 1024 * 1024;
const GRACEFUL_STOP_SECS: u64 = 90;
const FORCE_STOP_SECS: u64 = 10;

fn err(code: &str, msg: impl Into<String>, op: &str) -> AppError {
    AppError::new(code, msg, op)
}

fn io(e: std::io::Error, op: &str) -> AppError {
    err("IO_ERROR", format!("File operation failed: {e}"), op)
}

fn join_err(e: tauri::Error) -> AppError {
    err("TASK_FAILED", e.to_string(), "task")
}

pub(crate) fn lock<'a>(db: &'a DbState, op: &str) -> Result<MutexGuard<'a, Connection>, AppError> {
    db.0.lock().map_err(|_| err("DB_LOCK", "Database is unavailable", op))
}

// ------------------------------------------------------------------ in-memory VM state

/// Handles to VM processes started by this app run, plus the set of sandboxes with an operation in flight.
/// The database stays the source of truth for status; this only holds live process handles.
pub struct VmState {
    children: Mutex<HashMap<String, Child>>,
    busy: Mutex<HashSet<String>>,
}

impl VmState {
    pub fn new() -> Self {
        Self { children: Mutex::new(HashMap::new()), busy: Mutex::new(HashSet::new()) }
    }

    pub(crate) fn begin(&self, id: &str) -> Result<OpGuard<'_>, AppError> {
        let mut b = self.busy.lock().map_err(|_| err("STATE_LOCK", "VM state unavailable", "begin"))?;
        if !b.insert(id.to_string()) {
            return Err(err(
                "OPERATION_IN_PROGRESS",
                "Another operation on this sandbox is already running",
                "begin",
            ));
        }
        Ok(OpGuard { vm: self, id: id.to_string() })
    }

    fn is_busy(&self, id: &str) -> bool {
        self.busy.lock().map(|b| b.contains(id)).unwrap_or(true)
    }
}

pub struct OpGuard<'a> {
    vm: &'a VmState,
    id: String,
}

impl std::fmt::Debug for OpGuard<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OpGuard({})", self.id)
    }
}

impl Drop for OpGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut b) = self.vm.busy.lock() {
            b.remove(&self.id);
        }
    }
}

// ------------------------------------------------------------------ process helpers (Windows)

fn tasklist_has_qemu(stdout: &str) -> bool {
    stdout.to_ascii_lowercase().contains("qemu-system")
}

/// True only if `pid` is currently a running qemu-system process (guards against PID reuse).
pub fn pid_is_qemu(pid: u32) -> bool {
    let mut cmd = Command::new("tasklist");
    cmd.args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    match cmd.output() {
        Ok(o) => tasklist_has_qemu(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => false,
    }
}

fn is_alive(vm: &VmState, id: &str, pid: Option<u32>) -> bool {
    if let Ok(mut m) = vm.children.lock() {
        if let Some(c) = m.get_mut(id) {
            return matches!(c.try_wait(), Ok(None));
        }
    }
    pid.map(pid_is_qemu).unwrap_or(false)
}

fn drop_child(vm: &VmState, id: &str) {
    if let Ok(mut m) = vm.children.lock() {
        if let Some(mut c) = m.remove(id) {
            let _ = c.try_wait();
        }
    }
}

fn kill_vm(vm: &VmState, id: &str, pid: Option<u32>) {
    if let Ok(mut m) = vm.children.lock() {
        if let Some(c) = m.get_mut(id) {
            let _ = c.kill();
            return;
        }
    }
    if let Some(p) = pid {
        if pid_is_qemu(p) {
            let mut cmd = Command::new("taskkill");
            cmd.args(["/PID", &p.to_string(), "/F"]);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
            }
            let _ = cmd.output();
        }
    }
}

// ------------------------------------------------------------------ runtime file + QMP

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Runtime {
    pid: u32,
    qmp_port: u16,
}

fn runtime_path(dir: &Path) -> PathBuf {
    dir.join("runtime.json")
}

fn write_runtime(dir: &Path, rt: &Runtime) -> Result<(), AppError> {
    let json = serde_json::to_string(rt).map_err(|e| err("RUNTIME_WRITE", e.to_string(), "write_runtime"))?;
    fs::write(runtime_path(dir), json).map_err(|e| io(e, "write_runtime"))
}

fn read_runtime(dir: &Path) -> Option<Runtime> {
    let s = fs::read_to_string(runtime_path(dir)).ok()?;
    serde_json::from_str(&s).ok()
}

fn free_port() -> Result<u16, AppError> {
    let l = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| err("PORT_UNAVAILABLE", format!("No free local port: {e}"), "free_port"))?;
    l.local_addr()
        .map(|a| a.port())
        .map_err(|e| err("PORT_UNAVAILABLE", e.to_string(), "free_port"))
}

/// Ask the guest to shut down like pressing the power button (ACPI), via QEMU's QMP channel on localhost.
fn qmp_powerdown(port: u16) -> Result<(), AppError> {
    let op = "qmp_powerdown";
    let qerr = |m: String| err("QMP_FAILED", m, op);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3))
        .map_err(|e| err("QMP_CONNECT", format!("Could not reach the VM's control channel: {e}"), op))?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(s.try_clone().map_err(|e| io(e, op))?);

    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| qerr(format!("No greeting from VM: {e}")))?;

    for cmd in [
        "{\"execute\":\"qmp_capabilities\"}\n",
        "{\"execute\":\"system_powerdown\"}\n",
    ] {
        s.write_all(cmd.as_bytes()).map_err(|e| qerr(format!("Write failed: {e}")))?;
        loop {
            line.clear();
            let n = reader.read_line(&mut line).map_err(|e| qerr(format!("Read failed: {e}")))?;
            if n == 0 {
                return Err(qerr("Control channel closed unexpectedly".into()));
            }
            if line.contains("\"return\"") {
                break;
            }
            if line.contains("\"error\"") {
                return Err(qerr(format!("VM rejected the command: {}", line.trim())));
            }
            // otherwise it is an event line; keep reading
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ QEMU arguments

fn machine_for(accel: Option<&str>) -> Result<&'static str, AppError> {
    match accel.unwrap_or("whpx") {
        "whpx" => Ok("q35,accel=whpx,kernel-irqchip=off"),
        "tcg" => Ok("q35,accel=tcg"),
        _ => Err(err("ACCEL_INVALID", "Acceleration must be 'whpx' or 'tcg'", "machine_for")),
    }
}

fn safe_for_qemu(s: &str) -> bool {
    !s.contains(',') && !s.contains('\n') && !s.contains('\r')
}

/// Fixed argument array. No shell, no guest network, no shared folders, no USB passthrough.
/// The control channel listens on localhost only.
fn build_vm_args(
    overlay: &Path,
    cpu: u32,
    ram_mb: i64,
    name: &str,
    machine: &str,
    qmp_port: u16,
) -> Result<Vec<OsString>, AppError> {
    let op = "build_vm_args";
    let overlay_s = overlay.to_string_lossy();
    if !safe_for_qemu(&overlay_s) || !safe_for_qemu(name) {
        return Err(err("PATH_UNSAFE", "Paths and names used by the VM cannot contain commas or line breaks", op));
    }
    let s = |x: &str| OsString::from(x);
    Ok(vec![
        s("-machine"), s(machine),
        s("-cpu"), s("qemu64"),
        s("-smp"), s(&cpu.to_string()),
        s("-m"), s(&ram_mb.to_string()),
        s("-name"), s(name),
        s("-nic"), s("none"),
        s("-device"), s("qemu-xhci"),
        s("-device"), s("usb-tablet"),
        s("-boot"), s("order=c"),
        s("-drive"), s(&format!("file={},if=virtio,format=qcow2", overlay_s)),
        s("-qmp"), s(&format!("tcp:127.0.0.1:{qmp_port},server=on,wait=off")),
    ])
}

// ------------------------------------------------------------------ database helpers

pub(crate) fn get_sandbox(conn: &Connection, id: &str) -> Result<Sandbox, AppError> {
    database::list_sandboxes(conn)?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| err("NOT_FOUND", "Sandbox not found", "get_sandbox"))
}

pub(crate) fn get_image(conn: &Connection, id: &str) -> Result<OsImage, AppError> {
    conn.query_row(
        "SELECT id,name,version,image_path,image_format,checksum,verified,created_at FROM os_images WHERE id=?1",
        [id],
        |r| {
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
        },
    )
    .map_err(|_| err("IMAGE_NOT_FOUND", "OS image not found", "get_image"))
}

fn remove_dir_confined(base: &Path, dir: &Path) -> Result<(), AppError> {
    let checked = validation::resolve_inside(base, dir)?;
    fs::remove_dir_all(&checked).map_err(|e| io(e, "remove_dir_confined"))
}

fn read_tail(p: &Path) -> String {
    let b = fs::read(p).unwrap_or_default();
    let start = b.len().saturating_sub(2000);
    String::from_utf8_lossy(&b[start..]).to_string()
}

const DATA_FOLDER_NAME: &str = "DisposableOSSandbox";

/// Where everything lives: the database, settings, master images and sandbox disks.
/// It is under Local AppData and is not the folder Tauri names after the app identifier,
/// so removing or resetting the app's own folders does not touch it.
pub(crate) fn data_root_in(local_data: &Path) -> PathBuf {
    local_data.join(DATA_FOLDER_NAME)
}

pub(crate) fn app_data(app: &tauri::AppHandle, op: &str) -> Result<PathBuf, AppError> {
    let local = app
        .path()
        .local_data_dir()
        .map_err(|e| err("PATH_ERROR", e.to_string(), op))?;
    let root = data_root_in(&local);
    fs::create_dir_all(&root).map_err(|e| io(e, op))?;
    Ok(root)
}

fn revert_stopped(db: &DbState, id: &str, status: &str, msg: &str) {
    if let Ok(conn) = db.0.lock() {
        let _ = database::update_status(&conn, id, SandboxStatus::Stopped, None);
        let _ = database::log_activity(&conn, "vm_start", Some(id), status, msg);
    }
}

/// Make the database agree with reality: any sandbox marked running/starting/stopping whose
/// process is gone becomes Stopped (and the event is logged). Skips sandboxes with an operation in flight.
pub fn reconcile(conn: &Connection, vm: &VmState) -> Result<(), AppError> {
    for s in database::list_sandboxes(conn)? {
        if !matches!(s.status, SandboxStatus::Running | SandboxStatus::Starting | SandboxStatus::Stopping) {
            continue;
        }
        if vm.is_busy(&s.id) {
            continue;
        }
        if is_alive(vm, &s.id, s.process_id) {
            continue;
        }
        drop_child(vm, &s.id);
        if let Some(dir) = Path::new(&s.overlay_path).parent() {
            let _ = fs::remove_file(runtime_path(dir));
        }
        let was = s.status;
        database::update_status(conn, &s.id, SandboxStatus::Stopped, None)?;
        let (st, msg) = if was == SandboxStatus::Stopping {
            ("ok", "VM stopped")
        } else {
            ("warning", "VM process was no longer running; status corrected to stopped")
        };
        database::log_activity(conn, "vm_reconcile", Some(&s.id), st, msg)?;
    }
    Ok(())
}

// ------------------------------------------------------------------ commands

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSandboxRequest {
    pub name: String,
    pub description: Option<String>,
    pub image_id: String,
    pub cpu: u32,
    pub ram_mb: i64,
}

#[tauri::command]
pub async fn list_sandboxes(db: State<'_, DbState>, vm: State<'_, VmState>) -> Result<Vec<Sandbox>, AppError> {
    let conn = lock(&db, "list_sandboxes")?;
    reconcile(&conn, &vm)?;
    database::list_sandboxes(&conn)
}

#[tauri::command]
pub async fn create_sandbox(
    app: tauri::AppHandle,
    db: State<'_, DbState>,
    vm: State<'_, VmState>,
    req: CreateSandboxRequest,
) -> Result<Sandbox, AppError> {
    let op = "create_sandbox";
    validation::validate_sandbox_name(&req.name)?;
    let description = req.description.unwrap_or_default();
    if description.len() > 200 {
        return Err(err("DESC_LENGTH", "Description must be 200 characters or fewer", op));
    }
    let qemu_img = crate::qemu::find_qemu_img()
        .ok_or_else(|| err("QEMU_IMG_MISSING", "qemu-img was not found", op))?;
    let qi = PathBuf::from(&qemu_img.path);

    let data = app_data(&app, op)?;
    let images_dir = data.join("images");
    let sandboxes_dir = data.join("sandboxes");
    fs::create_dir_all(&sandboxes_dir).map_err(|e| io(e, op))?;

    let image = {
        let conn = lock(&db, op)?;
        get_image(&conn, &req.image_id)?
    };
    if !image.verified {
        return Err(err("IMAGE_UNVERIFIED", "Sandboxes can only be created from a verified image", op));
    }
    let master = disk_manager::plain_path(&validation::resolve_inside(&images_dir, Path::new(&image.image_path))?);
    let minfo = disk_manager::image_info(&qi, &master)?;
    if minfo.format != "qcow2" {
        return Err(err("MASTER_FORMAT", "Master image must be qcow2", op));
    }
    let disk_gb = ((minfo.virtual_size + GIB - 1) / GIB) as i64;

    let host = host::host_limits(&sandboxes_dir)?;
    validation::validate_resources(req.cpu, req.ram_mb, disk_gb, &host)?;

    let id = uuid::Uuid::new_v4().to_string();
    let _guard = vm.begin(&id)?;
    let dir_raw = sandboxes_dir.join(&id);
    fs::create_dir(&dir_raw).map_err(|e| io(e, op))?;
    let dir = match validation::resolve_inside(&sandboxes_dir, &dir_raw) {
        Ok(d) => disk_manager::plain_path(&d),
        Err(e) => {
            let _ = fs::remove_dir(&dir_raw);
            return Err(e);
        }
    };
    let overlay = dir.join("overlay.qcow2");

    let now = database::now();
    let sb = Sandbox {
        id: id.clone(),
        name: req.name.clone(),
        description,
        operating_system: image.name.clone(),
        os_version: image.version.clone(),
        base_image_id: image.id.clone(),
        overlay_path: overlay.to_string_lossy().to_string(),
        cpu_cores: req.cpu,
        ram_mb: req.ram_mb,
        disk_capacity_gb: disk_gb,
        status: SandboxStatus::Creating,
        process_id: None,
        created_at: now,
        updated_at: now,
        last_started_at: None,
    };

    // Insert first so a duplicate name fails before any files are made.
    let inserted = {
        let conn = lock(&db, op)?;
        database::insert_sandbox(&conn, &sb)
    };
    if let Err(e) = inserted {
        let _ = remove_dir_confined(&sandboxes_dir, &dir);
        return Err(e);
    }

    if let Err(e) = disk_manager::create_overlay(&qi, &master, &overlay) {
        if let Ok(conn) = db.0.lock() {
            let _ = conn.execute("DELETE FROM sandboxes WHERE id=?1", [&id]);
            let _ = database::log_activity(&conn, "sandbox_create", Some(&id), "error", &e.message);
        }
        let _ = remove_dir_confined(&sandboxes_dir, &dir);
        return Err(e);
    }

    let conn = lock(&db, op)?;
    database::update_status(&conn, &id, SandboxStatus::Stopped, None)?;
    database::log_activity(
        &conn,
        "sandbox_create",
        Some(&id),
        "ok",
        &format!("Sandbox '{}' created from '{} {}'", sb.name, image.name, image.version),
    )?;
    get_sandbox(&conn, &id)
}

#[tauri::command]
pub async fn start_sandbox(
    app: tauri::AppHandle,
    db: State<'_, DbState>,
    vm: State<'_, VmState>,
    id: String,
    accel: Option<String>,
) -> Result<Sandbox, AppError> {
    let op = "start_sandbox";
    {
        let conn = lock(&db, op)?;
        reconcile(&conn, &vm)?;
    }
    let _guard = vm.begin(&id)?;

    let data = app_data(&app, op)?;
    let images_dir = data.join("images");
    let sandboxes_dir = data.join("sandboxes");

    let (sb, image, other_running_ram) = {
        let conn = lock(&db, op)?;
        let sb = get_sandbox(&conn, &id)?;
        let image = get_image(&conn, &sb.base_image_id)?;
        let ram: i64 = database::list_sandboxes(&conn)?
            .iter()
            .filter(|o| {
                o.id != id
                    && matches!(o.status, SandboxStatus::Running | SandboxStatus::Starting | SandboxStatus::Stopping)
            })
            .map(|o| o.ram_mb)
            .sum();
        (sb, image, ram)
    };

    if sb.status != SandboxStatus::Stopped {
        return Err(err(
            "INVALID_STATE",
            format!("Sandbox is {}; it must be stopped to start", sb.status.as_str()),
            op,
        ));
    }
    let machine = machine_for(accel.as_deref())?;
    let qemu = crate::qemu::find_tool("qemu-system-x86_64.exe")
        .ok_or_else(|| err("QEMU_MISSING", "qemu-system-x86_64 was not found", op))?;
    let qemu_img = crate::qemu::find_qemu_img()
        .ok_or_else(|| err("QEMU_IMG_MISSING", "qemu-img was not found", op))?;
    let qi = PathBuf::from(&qemu_img.path);

    let overlay = disk_manager::plain_path(&validation::resolve_inside(&sandboxes_dir, Path::new(&sb.overlay_path))?);
    if !overlay.is_file() {
        return Err(err("OVERLAY_MISSING", "The sandbox's disk file is missing", op));
    }
    if !image.verified {
        return Err(err("IMAGE_UNVERIFIED", "The base image is not verified", op));
    }
    let master = disk_manager::plain_path(&validation::resolve_inside(&images_dir, Path::new(&image.image_path))?);
    disk_manager::verify_backing(&qi, &overlay, &master)?;

    let host = host::host_limits(&sandboxes_dir)?;
    validation::validate_resources(sb.cpu_cores, sb.ram_mb, sb.disk_capacity_gb, &host)?;
    if other_running_ram + sb.ram_mb > host.total_ram_mb * 3 / 4 {
        return Err(err(
            "RAM_TOTAL",
            "Starting this sandbox would use more than 75% of host RAM together with the sandboxes already running",
            op,
        ));
    }

    let dir = overlay
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| err("PATH_INVALID", "Invalid sandbox path", op))?;
    let port = free_port()?;
    let args = build_vm_args(&overlay, sb.cpu_cores, sb.ram_mb, &sb.name, machine, port)?;

    {
        let conn = lock(&db, op)?;
        database::update_status(&conn, &id, SandboxStatus::Starting, None)?;
    }

    let log = dir.join("qemu.log");
    let (log_out, log_err) = match File::create(&log).and_then(|f| f.try_clone().map(|g| (f, g))) {
        Ok(p) => p,
        Err(e) => {
            revert_stopped(&db, &id, "error", "Could not create the VM log file");
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
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            revert_stopped(&db, &id, "error", &format!("Could not start QEMU: {e}"));
            return Err(err("QEMU_LAUNCH", format!("Could not start QEMU: {e}"), op));
        }
    };

    // A bad configuration makes QEMU exit within a couple of seconds.
    tauri::async_runtime::spawn_blocking(|| std::thread::sleep(Duration::from_millis(2500)))
        .await
        .map_err(join_err)?;
    if let Ok(Some(st)) = child.try_wait() {
        let tail = read_tail(&log);
        revert_stopped(&db, &id, "error", &format!("QEMU exited immediately ({st})"));
        return Err(err(
            "QEMU_EXITED_EARLY",
            format!("QEMU exited immediately ({st}). Output: {}", tail.trim()),
            op,
        ));
    }

    let pid = child.id();
    if let Err(e) = write_runtime(&dir, &Runtime { pid, qmp_port: port }) {
        let _ = child.kill();
        let _ = child.wait();
        revert_stopped(&db, &id, "error", &e.message);
        return Err(e);
    }
    if let Ok(mut m) = vm.children.lock() {
        m.insert(id.clone(), child);
    }

    let conn = lock(&db, op)?;
    database::update_status(&conn, &id, SandboxStatus::Running, Some(pid))?;
    database::log_activity(&conn, "vm_start", Some(&id), "ok", &format!("VM started (pid {pid})"))?;
    get_sandbox(&conn, &id)
}

#[tauri::command]
pub async fn stop_sandbox(
    app: tauri::AppHandle,
    db: State<'_, DbState>,
    vm: State<'_, VmState>,
    id: String,
    force: Option<bool>,
) -> Result<Sandbox, AppError> {
    let op = "stop_sandbox";
    let force = force.unwrap_or(false);
    {
        let conn = lock(&db, op)?;
        reconcile(&conn, &vm)?;
    }
    let _guard = vm.begin(&id)?;
    let sandboxes_dir = app_data(&app, op)?.join("sandboxes");

    let sb = {
        let conn = lock(&db, op)?;
        get_sandbox(&conn, &id)?
    };
    if !matches!(sb.status, SandboxStatus::Running | SandboxStatus::Stopping) {
        return Err(err(
            "INVALID_STATE",
            format!("Sandbox is {}; only a running sandbox can be stopped", sb.status.as_str()),
            op,
        ));
    }
    let overlay = validation::resolve_inside(&sandboxes_dir, Path::new(&sb.overlay_path))?;
    let dir = overlay
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| err("PATH_INVALID", "Invalid sandbox path", op))?;

    if !force {
        let rt = read_runtime(&dir).ok_or_else(|| {
            err("NO_CONTROL_CHANNEL", "The VM's control channel is unknown. Use Force stop.", op)
        })?;
        qmp_powerdown(rt.qmp_port)?;
    }
    {
        let conn = lock(&db, op)?;
        database::update_status(&conn, &id, SandboxStatus::Stopping, sb.process_id)?;
    }
    if force {
        kill_vm(&vm, &id, sb.process_id);
    }

    let wait_secs = if force { FORCE_STOP_SECS } else { GRACEFUL_STOP_SECS };
    let start = Instant::now();
    let mut exited = false;
    while start.elapsed() < Duration::from_secs(wait_secs) {
        if !is_alive(&vm, &id, sb.process_id) {
            exited = true;
            break;
        }
        tauri::async_runtime::spawn_blocking(|| std::thread::sleep(Duration::from_millis(500)))
            .await
            .map_err(join_err)?;
    }
    if !exited {
        let (code, msg) = if force {
            ("KILL_FAILED", "The VM process did not exit after a forced stop")
        } else {
            (
                "STOP_TIMEOUT",
                "The guest did not shut down within 90 seconds. It is still stopping; you can use Force stop.",
            )
        };
        if let Ok(conn) = db.0.lock() {
            let _ = database::log_activity(&conn, "vm_stop", Some(&id), "error", msg);
        }
        return Err(err(code, msg, op));
    }

    drop_child(&vm, &id);
    let _ = fs::remove_file(runtime_path(&dir));
    let conn = lock(&db, op)?;
    database::update_status(&conn, &id, SandboxStatus::Stopped, None)?;
    database::log_activity(
        &conn,
        "vm_stop",
        Some(&id),
        "ok",
        if force { "VM force-stopped (power cut)" } else { "VM shut down gracefully" },
    )?;
    get_sandbox(&conn, &id)
}

#[tauri::command]
pub async fn get_vm_log(app: tauri::AppHandle, db: State<'_, DbState>, id: String) -> Result<String, AppError> {
    let op = "get_vm_log";
    let sandboxes_dir = app_data(&app, op)?.join("sandboxes");
    let sb = {
        let conn = lock(&db, op)?;
        get_sandbox(&conn, &id)?
    };
    let overlay = validation::resolve_inside(&sandboxes_dir, Path::new(&sb.overlay_path))?;
    let dir = overlay
        .parent()
        .ok_or_else(|| err("PATH_INVALID", "Invalid sandbox path", op))?;
    Ok(read_tail(&dir.join("qemu.log")))
}

#[derive(Debug, Serialize)]
pub struct VerifyReport {
    pub matches: bool,
    pub read_only: bool,
    pub expected: Option<String>,
    pub actual: String,
}

/// Hashes the master image and compares it to the checksum recorded when it was installed.
#[tauri::command]
pub async fn verify_image(
    app: tauri::AppHandle,
    db: State<'_, DbState>,
    image_id: String,
) -> Result<VerifyReport, AppError> {
    let op = "verify_image";
    let images_dir = app_data(&app, op)?.join("images");
    let image = {
        let conn = lock(&db, op)?;
        get_image(&conn, &image_id)?
    };
    let master = validation::resolve_inside(&images_dir, Path::new(&image.image_path))?;
    let read_only = fs::metadata(&master).map(|m| m.permissions().readonly()).unwrap_or(false);
    let m2 = master.clone();
    let actual = tauri::async_runtime::spawn_blocking(move || installer::sha256_file(&m2))
        .await
        .map_err(join_err)??;
    let matches = image.checksum.as_deref() == Some(actual.as_str());
    if let Ok(conn) = db.0.lock() {
        let _ = database::log_activity(
            &conn,
            "verify_image",
            None,
            if matches { "ok" } else { "error" },
            if matches { "Master image matches its recorded checksum" } else { "MASTER IMAGE CHECKSUM MISMATCH" },
        );
    }
    Ok(VerifyReport { matches, read_only, expected: image.checksum, actual })
}

#[cfg(test)]
mod data_root_tests {
    use super::*;

    #[test]
    fn data_root_is_a_separate_folder_under_local_app_data() {
        let root = data_root_in(Path::new("C:\\Users\\Ap\\AppData\\Local"));
        assert_eq!(root, PathBuf::from("C:\\Users\\Ap\\AppData\\Local\\DisposableOSSandbox"));
        assert!(!root.to_string_lossy().contains("com.disposableos.sandbox"));
        assert!(!root.to_string_lossy().contains("Roaming"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> OsImage {
        OsImage {
            id: "img1".into(), name: "Ubuntu".into(), version: "24.04".into(),
            image_path: "C:/images/master.qcow2".into(), image_format: "qcow2".into(),
            checksum: None, verified: true, created_at: 1,
        }
    }

    fn sandbox(id: &str, name: &str, status: SandboxStatus, pid: Option<u32>) -> Sandbox {
        Sandbox {
            id: id.into(), name: name.into(), description: String::new(),
            operating_system: "Ubuntu".into(), os_version: "24.04".into(),
            base_image_id: "img1".into(), overlay_path: format!("{id}/overlay.qcow2"),
            cpu_cores: 2, ram_mb: 2048, disk_capacity_gb: 25,
            status, process_id: pid,
            created_at: 1, updated_at: 1, last_started_at: None,
        }
    }

    #[test]
    fn tasklist_parsing() {
        assert!(tasklist_has_qemu("\"qemu-system-x86_64.exe\",\"1234\",\"Console\",\"1\",\"50,000 K\""));
        assert!(!tasklist_has_qemu("INFO: No tasks are running which match the specified criteria."));
        assert!(!tasklist_has_qemu("\"chrome.exe\",\"1234\",\"Console\",\"1\",\"50,000 K\""));
    }

    #[test]
    fn vm_args_are_isolated_and_safe() {
        let machine = machine_for(None).unwrap();
        let a: Vec<String> = build_vm_args(Path::new("C:\\vm\\overlay.qcow2"), 2, 2048, "Lab 1", machine, 4444)
            .unwrap()
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        let pos = a.iter().position(|x| x == "-nic").unwrap();
        assert_eq!(a[pos + 1], "none"); // no guest network
        assert!(a.iter().any(|x| x.contains("accel=whpx")));
        assert!(a.iter().any(|x| x == "tcp:127.0.0.1:4444,server=on,wait=off"));
        assert!(!a.iter().any(|x| x.contains("0.0.0.0")));
        assert!(!a.iter().any(|x| x == "-netdev" || x == "-virtfs" || x == "-usb" || x == "-fsdev"));
        assert!(a.iter().any(|x| x.starts_with("file=C:\\vm\\overlay.qcow2,")));
        assert_eq!(
            build_vm_args(Path::new("C:\\a,b\\o.qcow2"), 2, 2048, "x", machine, 1).unwrap_err().code,
            "PATH_UNSAFE"
        );
        assert_eq!(
            build_vm_args(Path::new("C:\\vm\\o.qcow2"), 2, 2048, "a,b", machine, 1).unwrap_err().code,
            "PATH_UNSAFE"
        );
    }

    #[test]
    fn accel_parsing_for_vm() {
        assert!(machine_for(Some("tcg")).unwrap().contains("accel=tcg"));
        assert!(machine_for(Some("kvm")).is_err());
    }

    #[test]
    fn runtime_file_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_runtime(tmp.path()).is_none());
        let rt = Runtime { pid: 4242, qmp_port: 5555 };
        write_runtime(tmp.path(), &rt).unwrap();
        assert_eq!(read_runtime(tmp.path()).unwrap(), rt);
    }

    #[test]
    fn one_operation_per_sandbox_at_a_time() {
        let vm = VmState::new();
        let g = vm.begin("a").unwrap();
        assert_eq!(vm.begin("a").unwrap_err().code, "OPERATION_IN_PROGRESS");
        assert!(vm.begin("b").is_ok()); // a different sandbox is independent
        drop(g);
        assert!(vm.begin("a").is_ok());
    }

    #[test]
    fn qmp_powerdown_sends_capabilities_then_powerdown() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.write_all(b"{\"QMP\": {\"version\": {}, \"capabilities\": []}}\n").unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut got = Vec::new();
            for _ in 0..2 {
                let mut l = String::new();
                r.read_line(&mut l).unwrap();
                got.push(l.trim().to_string());
                s.write_all(b"{\"return\": {}}\n").unwrap();
            }
            got
        });
        qmp_powerdown(port).unwrap();
        let got = h.join().unwrap();
        assert!(got[0].contains("qmp_capabilities"));
        assert!(got[1].contains("system_powerdown"));
    }

    #[test]
    fn qmp_refused_is_a_clear_error() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l); // nothing is listening now
        assert_eq!(qmp_powerdown(port).unwrap_err().code, "QMP_CONNECT");
    }

    #[test]
    fn reconcile_marks_dead_vms_stopped() {
        let conn = database::open_in_memory().unwrap();
        database::insert_image(&conn, &image()).unwrap();
        database::insert_sandbox(&conn, &sandbox("a", "Run", SandboxStatus::Running, None)).unwrap();
        database::insert_sandbox(&conn, &sandbox("b", "Start", SandboxStatus::Starting, None)).unwrap();
        database::insert_sandbox(&conn, &sandbox("c", "Stop", SandboxStatus::Stopping, None)).unwrap();
        database::insert_sandbox(&conn, &sandbox("d", "Idle", SandboxStatus::Stopped, None)).unwrap();
        let vm = VmState::new();
        reconcile(&conn, &vm).unwrap();
        for s in database::list_sandboxes(&conn).unwrap() {
            assert_eq!(s.status, SandboxStatus::Stopped, "{} should be stopped", s.name);
            assert_eq!(s.process_id, None);
        }
        let log = database::list_activity(&conn, 50).unwrap();
        assert_eq!(log.iter().filter(|e| e.operation == "vm_reconcile").count(), 3);
        assert_eq!(
            log.iter().filter(|e| e.operation == "vm_reconcile" && e.status == "warning").count(),
            2 // running and starting were unexpected; stopping was normal
        );
    }

    #[test]
    fn reconcile_skips_sandboxes_with_operation_in_flight() {
        let conn = database::open_in_memory().unwrap();
        database::insert_image(&conn, &image()).unwrap();
        database::insert_sandbox(&conn, &sandbox("a", "Busy", SandboxStatus::Starting, None)).unwrap();
        let vm = VmState::new();
        let _g = vm.begin("a").unwrap();
        reconcile(&conn, &vm).unwrap();
        assert_eq!(database::list_sandboxes(&conn).unwrap()[0].status, SandboxStatus::Starting);
    }
}