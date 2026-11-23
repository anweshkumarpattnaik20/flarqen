use crate::commands::DbState;
use crate::database;
use crate::disk_manager;
use crate::errors::AppError;
use crate::models::{Sandbox, SandboxStatus};
use crate::sandbox_manager::{app_data, get_image, get_sandbox, lock, reconcile, VmState};
use crate::validation;
use rusqlite::Connection;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::State;

fn err(code: &str, msg: impl Into<String>, op: &str) -> AppError {
    AppError::new(code, msg, op)
}

fn io(e: std::io::Error, op: &str) -> AppError {
    err("IO_ERROR", format!("File operation failed: {e}"), op)
}

fn join_err(e: tauri::Error) -> AppError {
    err("TASK_FAILED", e.to_string(), "task")
}

/// Deletes one sandbox's folder and nothing else. Refuses anything that is not exactly
/// `<sandboxes_dir>/<id>/overlay.qcow2`, so the master image, the sandboxes directory itself
/// and other sandboxes can never be removed through this path.
pub fn delete_sandbox_files(sandboxes_dir: &Path, overlay: &Path, id: &str) -> Result<(), AppError> {
    let op = "delete_sandbox_files";
    if overlay.file_name() != Some(OsStr::new("overlay.qcow2")) {
        return Err(err("PATH_UNEXPECTED", "Refusing to delete: unexpected disk file name", op));
    }
    let dir = overlay
        .parent()
        .ok_or_else(|| err("PATH_INVALID", "Invalid sandbox path", op))?;
    let checked = validation::resolve_inside(sandboxes_dir, dir)?;
    if checked.file_name() != Some(OsStr::new(id)) {
        return Err(err("PATH_MISMATCH", "Refusing to delete: folder does not belong to this sandbox", op));
    }
    if !checked.exists() {
        return Ok(()); // already gone; the database row can still be cleaned up
    }
    fs::remove_dir_all(&checked).map_err(|e| io(e, op))
}

/// Run at startup. Repairs sandboxes left half-finished by a crash or power loss.
pub fn recover_interrupted(conn: &Connection) -> Result<(), AppError> {
    for s in database::list_sandboxes(conn)? {
        match s.status {
            SandboxStatus::Resetting => {
                let overlay = PathBuf::from(&s.overlay_path);
                let new = overlay.with_extension("qcow2.new");
                let old = overlay.with_extension("qcow2.old");
                let mut restored = false;
                if !overlay.exists() && old.exists() && fs::rename(&old, &overlay).is_ok() {
                    restored = true;
                }
                let _ = fs::remove_file(&new);
                if overlay.exists() {
                    let _ = fs::remove_file(&old);
                    database::update_status(conn, &s.id, SandboxStatus::Stopped, None)?;
                    let msg = if restored {
                        "Interrupted reset: previous disk restored"
                    } else {
                        "Interrupted reset: existing disk kept and temporary files removed"
                    };
                    database::log_activity(conn, "recover_reset", Some(&s.id), "warning", msg)?;
                } else {
                    database::update_status(conn, &s.id, SandboxStatus::Error, None)?;
                    database::log_activity(
                        conn,
                        "recover_reset",
                        Some(&s.id),
                        "error",
                        "Interrupted reset: the disk file is missing. Delete this sandbox.",
                    )?;
                }
            }
            SandboxStatus::Creating => {
                database::update_status(conn, &s.id, SandboxStatus::Error, None)?;
                database::log_activity(
                    conn,
                    "recover_create",
                    Some(&s.id),
                    "error",
                    "Interrupted creation. Delete this sandbox and create it again.",
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn reset_sandbox(
    app: tauri::AppHandle,
    db: State<'_, DbState>,
    vm: State<'_, VmState>,
    id: String,
) -> Result<Sandbox, AppError> {
    let op = "reset_sandbox";
    {
        let conn = lock(&db, op)?;
        reconcile(&conn, &vm)?;
    }
    let _guard = vm.begin(&id)?;

    let data = app_data(&app, op)?;
    let images_dir = data.join("images");
    let sandboxes_dir = data.join("sandboxes");

    let (sb, image) = {
        let conn = lock(&db, op)?;
        let sb = get_sandbox(&conn, &id)?;
        let image = get_image(&conn, &sb.base_image_id)?;
        (sb, image)
    };
    if sb.status != SandboxStatus::Stopped {
        return Err(err(
            "INVALID_STATE",
            format!("Sandbox is {}; stop it before resetting", sb.status.as_str()),
            op,
        ));
    }
    if !image.verified {
        return Err(err("IMAGE_UNVERIFIED", "The base image is not verified", op));
    }
    let qemu_img = crate::qemu::find_qemu_img()
        .ok_or_else(|| err("QEMU_IMG_MISSING", "qemu-img was not found", op))?;
    let qi = PathBuf::from(&qemu_img.path);
    let overlay = disk_manager::plain_path(&validation::resolve_inside(&sandboxes_dir, Path::new(&sb.overlay_path))?);
    let master = disk_manager::plain_path(&validation::resolve_inside(&images_dir, Path::new(&image.image_path))?);

    {
        let conn = lock(&db, op)?;
        database::update_status(&conn, &id, SandboxStatus::Resetting, None)?;
    }

    let result = tauri::async_runtime::spawn_blocking(move || disk_manager::reset_overlay(&qi, &master, &overlay))
        .await
        .map_err(join_err)?;

    let conn = lock(&db, op)?;
    // Success or failure, the old disk is still in place or has been replaced, so the sandbox is stopped again.
    database::update_status(&conn, &id, SandboxStatus::Stopped, None)?;
    match result {
        Ok(()) => {
            database::log_activity(
                &conn,
                "sandbox_reset",
                Some(&id),
                "ok",
                &format!("Sandbox '{}' reset to the clean baseline", sb.name),
            )?;
            get_sandbox(&conn, &id)
        }
        Err(e) => {
            database::log_activity(
                &conn,
                "sandbox_reset",
                Some(&id),
                "error",
                &format!("Reset failed, previous disk kept: {}", e.message),
            )?;
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn delete_sandbox(
    app: tauri::AppHandle,
    db: State<'_, DbState>,
    vm: State<'_, VmState>,
    id: String,
) -> Result<String, AppError> {
    let op = "delete_sandbox";
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
    if !matches!(sb.status, SandboxStatus::Stopped | SandboxStatus::Error) {
        return Err(err(
            "INVALID_STATE",
            format!("Sandbox is {}; stop it before deleting", sb.status.as_str()),
            op,
        ));
    }

    let overlay = PathBuf::from(&sb.overlay_path);
    let sid = sb.id.clone();
    let files = tauri::async_runtime::spawn_blocking(move || delete_sandbox_files(&sandboxes_dir, &overlay, &sid))
        .await
        .map_err(join_err)?;

    let conn = lock(&db, op)?;
    if let Err(e) = files {
        database::log_activity(
            &conn,
            "sandbox_delete",
            Some(&id),
            "error",
            &format!("Delete failed, sandbox kept: {}", e.message),
        )?;
        return Err(e);
    }
    conn.execute("DELETE FROM sandboxes WHERE id=?1", [&id])?;
    let msg = format!("Sandbox '{}' deleted (master image and other sandboxes untouched)", sb.name);
    database::log_activity(&conn, "sandbox_delete", Some(&id), "ok", &msg)?;
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::OsImage;

    fn img() -> OsImage {
        OsImage {
            id: "img1".into(), name: "Ubuntu".into(), version: "24.04".into(),
            image_path: "C:/images/master.qcow2".into(), image_format: "qcow2".into(),
            checksum: None, verified: true, created_at: 1,
        }
    }

    fn sb(id: &str, name: &str, overlay: &Path, status: SandboxStatus) -> Sandbox {
        Sandbox {
            id: id.into(), name: name.into(), description: String::new(),
            operating_system: "Ubuntu".into(), os_version: "24.04".into(),
            base_image_id: "img1".into(), overlay_path: overlay.to_string_lossy().to_string(),
            cpu_cores: 2, ram_mb: 2048, disk_capacity_gb: 25,
            status, process_id: None, created_at: 1, updated_at: 1, last_started_at: None,
        }
    }

    struct Layout {
        _tmp: tempfile::TempDir,
        base: PathBuf,
        images: PathBuf,
    }

    fn layout() -> Layout {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("sandboxes");
        let images = tmp.path().join("images");
        for d in ["abc", "other"] {
            fs::create_dir_all(base.join(d)).unwrap();
            fs::write(base.join(d).join("overlay.qcow2"), b"disk").unwrap();
            fs::write(base.join(d).join("qemu.log"), b"log").unwrap();
        }
        fs::create_dir_all(&images).unwrap();
        fs::write(images.join("master.qcow2"), b"master").unwrap();
        Layout { _tmp: tmp, base, images }
    }

    #[test]
    fn delete_removes_only_the_selected_sandbox() {
        let l = layout();
        delete_sandbox_files(&l.base, &l.base.join("abc").join("overlay.qcow2"), "abc").unwrap();
        assert!(!l.base.join("abc").exists());
        assert!(l.base.join("other").join("overlay.qcow2").exists()); // other sandbox untouched
        assert!(l.images.join("master.qcow2").exists()); // master untouched
    }

    #[test]
    fn delete_refuses_wrong_id_and_outside_paths() {
        let l = layout();
        // overlay belongs to "abc" but the id says "other"
        let e = delete_sandbox_files(&l.base, &l.base.join("abc").join("overlay.qcow2"), "other").unwrap_err();
        assert_eq!(e.code, "PATH_MISMATCH");
        assert!(l.base.join("abc").exists());
        // a path in the images directory (where the master lives)
        let e = delete_sandbox_files(&l.base, &l.images.join("overlay.qcow2"), "images").unwrap_err();
        assert_eq!(e.code, "PATH_OUTSIDE");
        assert!(l.images.join("master.qcow2").exists());
        // the sandboxes directory itself
        let e = delete_sandbox_files(&l.base, &l.base.join("overlay.qcow2"), "sandboxes").unwrap_err();
        assert_eq!(e.code, "PATH_OUTSIDE");
        assert!(l.base.exists() && l.base.join("other").exists());
        // traversal
        let sneaky = l.base.join("abc").join("..").join("other").join("overlay.qcow2");
        assert!(delete_sandbox_files(&l.base, &sneaky, "other").is_err());
        assert!(l.base.join("other").exists());
    }

    #[test]
    fn delete_refuses_unexpected_file_name() {
        let l = layout();
        let e = delete_sandbox_files(&l.base, &l.base.join("abc").join("master.qcow2"), "abc").unwrap_err();
        assert_eq!(e.code, "PATH_UNEXPECTED");
        assert!(l.base.join("abc").exists());
    }

    #[test]
    fn delete_of_missing_folder_is_ok() {
        let l = layout();
        fs::remove_dir_all(l.base.join("abc")).unwrap();
        delete_sandbox_files(&l.base, &l.base.join("abc").join("overlay.qcow2"), "abc").unwrap();
    }

    #[test]
    fn recover_keeps_existing_disk_and_cleans_temp_files() {
        let tmp = tempfile::tempdir().unwrap();
        let conn = database::open_in_memory().unwrap();
        database::insert_image(&conn, &img()).unwrap();
        let ov = tmp.path().join("overlay.qcow2");
        fs::write(&ov, b"current").unwrap();
        fs::write(ov.with_extension("qcow2.new"), b"n").unwrap();
        fs::write(ov.with_extension("qcow2.old"), b"o").unwrap();
        database::insert_sandbox(&conn, &sb("a", "A", &ov, SandboxStatus::Resetting)).unwrap();
        recover_interrupted(&conn).unwrap();
        let s = &database::list_sandboxes(&conn).unwrap()[0];
        assert_eq!(s.status, SandboxStatus::Stopped);
        assert_eq!(fs::read(&ov).unwrap(), b"current");
        assert!(!ov.with_extension("qcow2.new").exists());
        assert!(!ov.with_extension("qcow2.old").exists());
    }

    #[test]
    fn recover_restores_old_disk_when_swap_was_interrupted() {
        let tmp = tempfile::tempdir().unwrap();
        let conn = database::open_in_memory().unwrap();
        database::insert_image(&conn, &img()).unwrap();
        let ov = tmp.path().join("overlay.qcow2");
        fs::write(ov.with_extension("qcow2.old"), b"old-data").unwrap();
        database::insert_sandbox(&conn, &sb("a", "A", &ov, SandboxStatus::Resetting)).unwrap();
        recover_interrupted(&conn).unwrap();
        assert_eq!(database::list_sandboxes(&conn).unwrap()[0].status, SandboxStatus::Stopped);
        assert_eq!(fs::read(&ov).unwrap(), b"old-data");
    }

    #[test]
    fn recover_marks_unrecoverable_reset_as_error() {
        let tmp = tempfile::tempdir().unwrap();
        let conn = database::open_in_memory().unwrap();
        database::insert_image(&conn, &img()).unwrap();
        let ov = tmp.path().join("overlay.qcow2");
        database::insert_sandbox(&conn, &sb("a", "A", &ov, SandboxStatus::Resetting)).unwrap();
        recover_interrupted(&conn).unwrap();
        assert_eq!(database::list_sandboxes(&conn).unwrap()[0].status, SandboxStatus::Error);
    }

    #[test]
    fn recover_marks_interrupted_creation_as_error() {
        let tmp = tempfile::tempdir().unwrap();
        let conn = database::open_in_memory().unwrap();
        database::insert_image(&conn, &img()).unwrap();
        let ov = tmp.path().join("overlay.qcow2");
        database::insert_sandbox(&conn, &sb("a", "A", &ov, SandboxStatus::Creating)).unwrap();
        recover_interrupted(&conn).unwrap();
        assert_eq!(database::list_sandboxes(&conn).unwrap()[0].status, SandboxStatus::Error);
        let log = database::list_activity(&conn, 10).unwrap();
        assert!(log.iter().any(|e| e.operation == "recover_create"));
    }
}