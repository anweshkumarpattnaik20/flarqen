//! Snapshots: named copies of a stopped sandbox's disk that it can later be restored to.
use crate::commands::DbState;
use crate::database;
use crate::disk_manager;
use crate::errors::AppError;
use crate::models::{Sandbox, SandboxStatus};
use crate::sandbox_manager::{app_data, get_sandbox, lock, reconcile, VmState};
use crate::validation;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::State;

const MAX_SNAPSHOTS: i64 = 5;

#[derive(Serialize, Clone, Debug)]
pub struct SnapshotInfo {
    pub id: String,
    pub sandbox_id: String,
    pub name: String,
    pub created_at: i64,
    pub size_bytes: i64,
}

fn err(code: &str, msg: impl Into<String>, op: &str) -> AppError {
    AppError::new(code, msg, op)
}

fn io(e: std::io::Error, op: &str) -> AppError {
    err("IO_ERROR", format!("File operation failed: {e}"), op)
}

fn join_err(e: tauri::Error) -> AppError {
    err("TASK_FAILED", e.to_string(), "task")
}

/// 1-40 characters, no control characters and none of the characters Windows forbids in names.
fn validate_snapshot_name(raw: &str) -> Result<String, AppError> {
    let name = raw.trim().to_string();
    let n = name.chars().count();
    if n == 0 || n > 40 {
        return Err(err("SNAPSHOT_NAME", "Snapshot name must be 1 to 40 characters", "snapshot"));
    }
    if name.chars().any(|c| c.is_control() || "<>:\"/\\|?*".contains(c)) {
        return Err(err("SNAPSHOT_NAME", "Snapshot name contains characters that are not allowed", "snapshot"));
    }
    Ok(name)
}

/// `<sandbox folder>/snapshots/<id>.qcow2`, only for ids that look exactly like the ones this app makes.
fn snapshot_file(sandbox_dir: &Path, snap_id: &str) -> Result<PathBuf, AppError> {
    let ok = snap_id.len() == 36 && snap_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    if !ok {
        return Err(err("SNAPSHOT_ID", "Invalid snapshot id", "snapshot"));
    }
    Ok(sandbox_dir.join("snapshots").join(format!("{snap_id}.qcow2")))
}

/// Resolves the sandbox's disk and folder, refusing anything that is not `<sandboxes>/<id>/overlay.qcow2`.
fn disk_and_dir(sandboxes_dir: &Path, sb: &Sandbox, op: &str) -> Result<(PathBuf, PathBuf), AppError> {
    let overlay = disk_manager::plain_path(&validation::resolve_inside(sandboxes_dir, Path::new(&sb.overlay_path))?);
    let name_ok = overlay.file_name().map(|n| n == "overlay.qcow2").unwrap_or(false);
    let dir = overlay.parent().map(|p| p.to_path_buf());
    let dir_ok = dir.as_ref().and_then(|d| d.file_name()).map(|n| n == sb.id.as_str()).unwrap_or(false);
    match dir {
        Some(d) if name_ok && dir_ok => Ok((overlay, d)),
        _ => Err(err("SOURCE_DISK", "The sandbox disk is not where expected", op)),
    }
}

fn list_rows(conn: &Connection, sandbox_id: &str) -> Result<Vec<SnapshotInfo>, AppError> {
    let mut st = conn.prepare(
        "SELECT id, sandbox_id, name, created_at, size_bytes FROM sandbox_snapshots
         WHERE sandbox_id = ?1 ORDER BY created_at DESC, rowid DESC",
    )?;
    let rows = st.query_map([sandbox_id], |r| {
        Ok(SnapshotInfo { id: r.get(0)?, sandbox_id: r.get(1)?, name: r.get(2)?, created_at: r.get(3)?, size_bytes: r.get(4)? })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn get_row(conn: &Connection, sandbox_id: &str, snap_id: &str) -> Result<SnapshotInfo, AppError> {
    list_rows(conn, sandbox_id)?
        .into_iter()
        .find(|s| s.id == snap_id)
        .ok_or_else(|| err("NOT_FOUND", "Snapshot not found", "snapshot"))
}

/// Replaces `overlay` with `overlay.qcow2.new`. If the second step fails the original is put back.
/// The file names match what startup recovery looks for after an interrupted reset.
fn swap_in_new(overlay: &Path) -> Result<(), AppError> {
    let op = "restore_snapshot";
    let new = overlay.with_extension("qcow2.new");
    let old = overlay.with_extension("qcow2.old");
    let _ = fs::remove_file(&old);
    if let Err(e) = fs::rename(overlay, &old) {
        let _ = fs::remove_file(&new);
        return Err(io(e, op));
    }
    if let Err(e) = fs::rename(&new, overlay) {
        let _ = fs::rename(&old, overlay);
        let _ = fs::remove_file(&new);
        return Err(io(e, op));
    }
    let _ = fs::remove_file(&old);
    Ok(())
}

/// Copies the snapshot next to the disk, checks it, then swaps it in.
fn restore_files(qi: &Path, snap: &Path, overlay: &Path) -> Result<(), AppError> {
    let op = "restore_snapshot";
    let new = overlay.with_extension("qcow2.new");
    let old = overlay.with_extension("qcow2.old");
    let _ = fs::remove_file(&new);
    let _ = fs::remove_file(&old);
    if let Err(e) = fs::copy(snap, &new) {
        let _ = fs::remove_file(&new);
        return Err(io(e, op));
    }
    match disk_manager::image_info(qi, &new) {
        Ok(info) if info.format == "qcow2" => {}
        Ok(_) => {
            let _ = fs::remove_file(&new);
            return Err(err("SNAPSHOT_INVALID", "The snapshot file is not a qcow2 image", op));
        }
        Err(e) => {
            let _ = fs::remove_file(&new);
            return Err(e);
        }
    }
    swap_in_new(overlay)
}

#[tauri::command]
pub async fn list_snapshots(db: State<'_, DbState>, id: String) -> Result<Vec<SnapshotInfo>, AppError> {
    let conn = lock(&db, "list_snapshots")?;
    list_rows(&conn, &id)
}

#[tauri::command]
pub async fn create_snapshot(
    app: tauri::AppHandle,
    db: State<'_, DbState>,
    vm: State<'_, VmState>,
    id: String,
    name: String,
) -> Result<SnapshotInfo, AppError> {
    let op = "create_snapshot";
    let name = validate_snapshot_name(&name)?;
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
    if sb.status != SandboxStatus::Stopped {
        return Err(err("NOT_STOPPED", "Stop the sandbox before taking a snapshot", op));
    }
    let (overlay, dir) = disk_and_dir(&sandboxes_dir, &sb, op)?;
    let qi = crate::qemu::find_qemu_img()
        .map(|t| PathBuf::from(t.path))
        .ok_or_else(|| err("QEMU_IMG_MISSING", "qemu-img was not found", op))?;

    let snap_id = uuid::Uuid::new_v4().to_string();
    let file = snapshot_file(&dir, &snap_id)?;
    let created_at = database::now();
    {
        let conn = lock(&db, op)?;
        let have: i64 = conn.query_row("SELECT COUNT(*) FROM sandbox_snapshots WHERE sandbox_id = ?1", [&id], |r| r.get(0))?;
        if have >= MAX_SNAPSHOTS {
            return Err(err("SNAPSHOT_LIMIT", format!("A sandbox can keep at most {MAX_SNAPSHOTS} snapshots. Delete one first."), op));
        }
        // Insert first so a duplicate name fails before any large file is copied.
        conn.execute(
            "INSERT INTO sandbox_snapshots (id, sandbox_id, name, created_at, size_bytes) VALUES (?1, ?2, ?3, ?4, 0)",
            params![snap_id, id, name, created_at],
        )
        .map_err(|_| err("SNAPSHOT_NAME", "This sandbox already has a snapshot with that name", op))?;
    }

    let undo = |msg: &str| {
        if let Ok(conn) = db.0.lock() {
            let _ = conn.execute("DELETE FROM sandbox_snapshots WHERE id = ?1", [&snap_id]);
            let _ = database::log_activity(&conn, "snapshot_create", Some(&id), "error", msg);
        }
        let _ = fs::remove_file(&file);
        let _ = fs::remove_file(file.with_extension("qcow2.tmp"));
    };

    let (from, to, tmp) = (overlay.clone(), file.clone(), file.with_extension("qcow2.tmp"));
    let copied = tauri::async_runtime::spawn_blocking(move || -> std::io::Result<u64> {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&from, &tmp)?;
        fs::rename(&tmp, &to)?;
        fs::metadata(&to).map(|m| m.len())
    })
    .await
    .map_err(join_err)?;
    let size = match copied {
        Ok(n) => n,
        Err(e) => {
            let msg = format!("Copying the disk failed: {e}");
            undo(&msg);
            return Err(err("COPY_FAILED", msg, op));
        }
    };

    match disk_manager::image_info(&qi, &file) {
        Ok(info) if info.format == "qcow2" => {}
        Ok(_) => {
            undo("The snapshot file is not a qcow2 image");
            return Err(err("SNAPSHOT_INVALID", "The snapshot file is not a qcow2 image", op));
        }
        Err(e) => {
            undo(&e.message);
            return Err(e);
        }
    }

    let conn = lock(&db, op)?;
    conn.execute("UPDATE sandbox_snapshots SET size_bytes = ?1 WHERE id = ?2", params![size as i64, snap_id])?;
    database::log_activity(&conn, "snapshot_create", Some(&id), "ok", &format!("Snapshot '{}' saved for '{}'", name, sb.name))?;
    get_row(&conn, &id, &snap_id)
}

#[tauri::command]
pub async fn restore_snapshot(
    app: tauri::AppHandle,
    db: State<'_, DbState>,
    vm: State<'_, VmState>,
    id: String,
    snapshot_id: String,
) -> Result<Sandbox, AppError> {
    let op = "restore_snapshot";
    {
        let conn = lock(&db, op)?;
        reconcile(&conn, &vm)?;
    }
    let _guard = vm.begin(&id)?;
    let sandboxes_dir = app_data(&app, op)?.join("sandboxes");

    let (sb, snap) = {
        let conn = lock(&db, op)?;
        (get_sandbox(&conn, &id)?, get_row(&conn, &id, &snapshot_id)?)
    };
    if sb.status != SandboxStatus::Stopped {
        return Err(err("NOT_STOPPED", "Stop the sandbox before restoring a snapshot", op));
    }
    let (overlay, dir) = disk_and_dir(&sandboxes_dir, &sb, op)?;
    let file = snapshot_file(&dir, &snap.id)?;
    if !file.is_file() {
        return Err(err("SNAPSHOT_MISSING", "The snapshot file is missing", op));
    }
    let qi = crate::qemu::find_qemu_img()
        .map(|t| PathBuf::from(t.path))
        .ok_or_else(|| err("QEMU_IMG_MISSING", "qemu-img was not found", op))?;

    {
        // Same state as a reset, so startup recovery repairs an interrupted restore the same way.
        let conn = lock(&db, op)?;
        database::update_status(&conn, &id, SandboxStatus::Resetting, None)?;
    }
    let result = tauri::async_runtime::spawn_blocking(move || restore_files(&qi, &file, &overlay))
        .await
        .map_err(join_err)?;

    let conn = lock(&db, op)?;
    database::update_status(&conn, &id, SandboxStatus::Stopped, None)?;
    match result {
        Ok(()) => {
            database::log_activity(
                &conn,
                "snapshot_restore",
                Some(&id),
                "ok",
                &format!("Sandbox '{}' restored to snapshot '{}'", sb.name, snap.name),
            )?;
            get_sandbox(&conn, &id)
        }
        Err(e) => {
            database::log_activity(
                &conn,
                "snapshot_restore",
                Some(&id),
                "error",
                &format!("Restore failed, previous disk kept: {}", e.message),
            )?;
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn delete_snapshot(
    app: tauri::AppHandle,
    db: State<'_, DbState>,
    vm: State<'_, VmState>,
    id: String,
    snapshot_id: String,
) -> Result<String, AppError> {
    let op = "delete_snapshot";
    let _guard = vm.begin(&id)?;
    let sandboxes_dir = app_data(&app, op)?.join("sandboxes");
    let (sb, snap) = {
        let conn = lock(&db, op)?;
        (get_sandbox(&conn, &id)?, get_row(&conn, &id, &snapshot_id)?)
    };
    let (_, dir) = disk_and_dir(&sandboxes_dir, &sb, op)?;
    let file = snapshot_file(&dir, &snap.id)?;
    match fs::remove_file(&file) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io(e, op)),
    }
    let conn = lock(&db, op)?;
    conn.execute("DELETE FROM sandbox_snapshots WHERE id = ?1", [&snap.id])?;
    let msg = format!("Snapshot '{}' of '{}' deleted", snap.name, sb.name);
    database::log_activity(&conn, "snapshot_delete", Some(&id), "ok", &msg)?;
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::OsImage;

    fn db_with(ids: &[&str]) -> Connection {
        let c = database::open_in_memory().unwrap();
        database::insert_image(
            &c,
            &OsImage {
                id: "img1".into(), name: "Ubuntu".into(), version: "24.04".into(),
                image_path: "C:/images/u.qcow2".into(), image_format: "qcow2".into(),
                checksum: None, verified: true, created_at: 1,
            },
        )
        .unwrap();
        for id in ids {
            database::insert_sandbox(
                &c,
                &Sandbox {
                    id: (*id).into(), name: format!("Lab {id}"), description: String::new(),
                    operating_system: "Ubuntu".into(), os_version: "24.04".into(),
                    base_image_id: "img1".into(), overlay_path: format!("{id}/overlay.qcow2"),
                    cpu_cores: 2, ram_mb: 2048, disk_capacity_gb: 20,
                    status: SandboxStatus::Stopped, process_id: None,
                    created_at: 1, updated_at: 1, last_started_at: None,
                },
            )
            .unwrap();
        }
        c
    }

    fn add(c: &Connection, sid: &str, snap: &str, name: &str, at: i64) -> rusqlite::Result<usize> {
        c.execute(
            "INSERT INTO sandbox_snapshots (id, sandbox_id, name, created_at, size_bytes) VALUES (?1, ?2, ?3, ?4, 10)",
            params![snap, sid, name, at],
        )
    }

    #[test]
    fn names_are_checked() {
        assert_eq!(validate_snapshot_name("  Clean install ").unwrap(), "Clean install");
        assert!(validate_snapshot_name("").is_err());
        assert!(validate_snapshot_name("   ").is_err());
        assert!(validate_snapshot_name(&"x".repeat(41)).is_err());
        assert!(validate_snapshot_name("a/b").is_err());
        assert!(validate_snapshot_name("bad\nname").is_err());
    }

    #[test]
    fn snapshot_paths_only_for_real_ids() {
        let dir = Path::new("data").join("sandboxes").join("abc");
        let id = "123e4567-e89b-12d3-a456-426614174000";
        let f = snapshot_file(&dir, id).unwrap();
        assert!(f.starts_with(dir.join("snapshots")));
        assert!(snapshot_file(&dir, "../../etc/passwd").is_err());
        assert!(snapshot_file(&dir, "abc").is_err());
        assert!(snapshot_file(&dir, "123e4567-e89b-12d3-a456-42661417400/").is_err());
    }

    #[test]
    fn swap_replaces_the_disk_and_leaves_no_temp_files() {
        let d = tempfile::tempdir().unwrap();
        let overlay = d.path().join("overlay.qcow2");
        fs::write(&overlay, b"current").unwrap();
        fs::write(overlay.with_extension("qcow2.new"), b"from snapshot").unwrap();
        swap_in_new(&overlay).unwrap();
        assert_eq!(fs::read(&overlay).unwrap(), b"from snapshot");
        assert!(!overlay.with_extension("qcow2.new").exists());
        assert!(!overlay.with_extension("qcow2.old").exists());
    }

    #[test]
    fn failed_swap_keeps_the_current_disk() {
        let d = tempfile::tempdir().unwrap();
        let overlay = d.path().join("overlay.qcow2");
        fs::write(&overlay, b"current").unwrap();
        // no .new file exists, so the second rename fails and the first must be undone
        assert!(swap_in_new(&overlay).is_err());
        assert_eq!(fs::read(&overlay).unwrap(), b"current");
        assert!(!overlay.with_extension("qcow2.old").exists());
    }

    #[test]
    fn snapshot_names_are_unique_per_sandbox_ignoring_case() {
        let c = db_with(&["a", "b"]);
        add(&c, "a", "s1", "Clean", 1).unwrap();
        assert!(add(&c, "a", "s2", "clean", 2).is_err());
        add(&c, "b", "s3", "Clean", 3).unwrap(); // another sandbox may reuse the name
    }

    #[test]
    fn snapshots_list_newest_first_and_go_with_their_sandbox() {
        let c = db_with(&["a", "b"]);
        add(&c, "a", "s1", "one", 1).unwrap();
        add(&c, "a", "s2", "two", 2).unwrap();
        add(&c, "b", "s3", "other", 3).unwrap();
        let l = list_rows(&c, "a").unwrap();
        assert_eq!(l.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["two", "one"]);
        c.execute("DELETE FROM sandboxes WHERE id = 'a'", []).unwrap();
        assert!(list_rows(&c, "a").unwrap().is_empty());
        assert_eq!(list_rows(&c, "b").unwrap().len(), 1);
        assert!(get_row(&c, "b", "nope").is_err());
    }
}