//! Duplicate a stopped sandbox: copies its disk (current state) into a new, independent sandbox.
use crate::commands::DbState;
use crate::database;
use crate::disk_manager;
use crate::errors::AppError;
use crate::models::{Sandbox, SandboxStatus};
use crate::sandbox_manager::{app_data, get_sandbox, lock, VmState};
use crate::validation;
use std::fs;
use std::path::Path;
use tauri::State;

fn err(code: &str, msg: impl Into<String>, op: &str) -> AppError {
    AppError::new(code, msg, op)
}

/// True only for `<...>/sandboxes/<new_id>`, the folder this command just created.
fn is_new_sandbox_dir(dir: &Path, new_id: &str) -> bool {
    dir.file_name().map(|n| n == new_id).unwrap_or(false)
        && dir.parent().and_then(|p| p.file_name()).map(|n| n == "sandboxes").unwrap_or(false)
}

/// Removes the half-made copy: its database row and its folder (and nothing else).
fn undo(db: &DbState, dir: &Path, new_id: &str, msg: &str) {
    if let Ok(conn) = db.0.lock() {
        let _ = conn.execute("DELETE FROM sandboxes WHERE id=?1", [new_id]);
        let _ = database::log_activity(&conn, "sandbox_duplicate", Some(new_id), "error", msg);
    }
    if is_new_sandbox_dir(dir, new_id) {
        let _ = fs::remove_dir_all(dir);
    }
}

#[tauri::command]
pub async fn duplicate_sandbox(
    app: tauri::AppHandle,
    db: State<'_, DbState>,
    vm: State<'_, VmState>,
    id: String,
    new_name: String,
) -> Result<Sandbox, AppError> {
    let op = "duplicate_sandbox";
    validation::validate_sandbox_name(&new_name)?;
    let sandboxes_dir = app_data(&app, op)?.join("sandboxes");

    // Block start/reset/delete of the source, and of the new sandbox, while the copy runs.
    let new_id = uuid::Uuid::new_v4().to_string();
    let _src_guard = vm.begin(&id)?;
    let _new_guard = vm.begin(&new_id)?;

    let source = {
        let conn = lock(&db, op)?;
        get_sandbox(&conn, &id)?
    };
    if source.status != SandboxStatus::Stopped {
        return Err(err("NOT_STOPPED", "Stop the sandbox before duplicating it", op));
    }

    let src_overlay = disk_manager::plain_path(&validation::resolve_inside(&sandboxes_dir, Path::new(&source.overlay_path))?);
    let name_ok = src_overlay.file_name().map(|n| n == "overlay.qcow2").unwrap_or(false);
    let dir_ok = src_overlay.parent().and_then(|p| p.file_name()).map(|n| n == id.as_str()).unwrap_or(false);
    if !name_ok || !dir_ok || !src_overlay.is_file() {
        return Err(err("SOURCE_DISK", "The source sandbox disk was not found where expected", op));
    }
    let qi = crate::qemu::find_qemu_img()
        .map(|t| std::path::PathBuf::from(t.path))
        .ok_or_else(|| err("QEMU_IMG_MISSING", "qemu-img was not found", op))?;

    let dir_raw = sandboxes_dir.join(&new_id);
    fs::create_dir(&dir_raw).map_err(|e| err("IO_ERROR", format!("Could not create the folder: {e}"), op))?;
    let dir = match validation::resolve_inside(&sandboxes_dir, &dir_raw) {
        Ok(d) => disk_manager::plain_path(&d),
        Err(e) => {
            let _ = fs::remove_dir(&dir_raw);
            return Err(e);
        }
    };
    let overlay = dir.join("overlay.qcow2");

    let now = database::now();
    let copy = Sandbox {
        id: new_id.clone(),
        name: new_name.clone(),
        description: source.description.clone(),
        operating_system: source.operating_system.clone(),
        os_version: source.os_version.clone(),
        base_image_id: source.base_image_id.clone(),
        overlay_path: overlay.to_string_lossy().to_string(),
        cpu_cores: source.cpu_cores,
        ram_mb: source.ram_mb,
        disk_capacity_gb: source.disk_capacity_gb,
        status: SandboxStatus::Creating,
        process_id: None,
        created_at: now,
        updated_at: now,
        last_started_at: None,
    };

    // Insert first so a duplicate name fails before any large file is copied.
    let inserted = {
        let conn = lock(&db, op)?;
        database::insert_sandbox(&conn, &copy)
    };
    if let Err(e) = inserted {
        if is_new_sandbox_dir(&dir, &new_id) {
            let _ = fs::remove_dir(&dir);
        }
        return Err(e);
    }

    // The copy can be gigabytes: do it off the async thread.
    let (from, to) = (src_overlay.clone(), overlay.clone());
    let copied = tauri::async_runtime::spawn_blocking(move || fs::copy(&from, &to))
        .await
        .map_err(|e| err("TASK_FAILED", e.to_string(), op))?;
    if let Err(e) = copied {
        let msg = format!("Copying the disk failed: {e}");
        undo(&db, &dir, &new_id, &msg);
        return Err(err("COPY_FAILED", msg, op));
    }

    // The copy must still be a valid qcow2 disk before the sandbox is usable.
    match disk_manager::image_info(&qi, &overlay) {
        Ok(info) if info.format == "qcow2" => {}
        Ok(_) => {
            let msg = "The copied disk is not a qcow2 image";
            undo(&db, &dir, &new_id, msg);
            return Err(err("COPY_INVALID", msg, op));
        }
        Err(e) => {
            undo(&db, &dir, &new_id, &e.message);
            return Err(e);
        }
    }

    let conn = lock(&db, op)?;
    database::update_status(&conn, &new_id, SandboxStatus::Stopped, None)?;
    database::log_activity(
        &conn,
        "sandbox_duplicate",
        Some(&new_id),
        "ok",
        &format!("Sandbox '{}' duplicated from '{}'", new_name, source.name),
    )?;
    get_sandbox(&conn, &new_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_new_sandbox_folder_may_be_removed() {
        let base = Path::new("data").join("sandboxes");
        assert!(is_new_sandbox_dir(&base.join("abc"), "abc"));
        assert!(!is_new_sandbox_dir(&base.join("abc"), "other"));
        assert!(!is_new_sandbox_dir(&base, "sandboxes"));
        assert!(!is_new_sandbox_dir(&Path::new("data").join("images").join("abc"), "abc"));
    }
}