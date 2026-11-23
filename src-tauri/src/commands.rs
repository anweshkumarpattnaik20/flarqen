use crate::database;
use crate::disk_manager::{self, SelfTestReport};
use crate::errors::AppError;
use crate::models::{ActivityEntry, Sandbox};
use crate::sandbox_manager::app_data;
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;
use tauri::State;

pub struct DbState(pub Mutex<Connection>);

fn lock<'a>(db: &'a State<DbState>, op: &str) -> Result<std::sync::MutexGuard<'a, Connection>, AppError> {
    db.0.lock().map_err(|_| AppError::new("DB_LOCK", "Database is unavailable", op))
}

#[tauri::command]
pub fn get_sandbox_list(db: State<DbState>) -> Result<Vec<Sandbox>, AppError> {
    let conn = lock(&db, "get_sandbox_list")?;
    database::list_sandboxes(&conn)
}

#[tauri::command]
pub fn get_activity_log(db: State<DbState>, limit: Option<i64>) -> Result<Vec<ActivityEntry>, AppError> {
    let conn = lock(&db, "get_activity_log")?;
    database::list_activity(&conn, limit.unwrap_or(50))
}

#[tauri::command]
pub async fn run_disk_selftest(app: tauri::AppHandle, db: State<'_, DbState>) -> Result<SelfTestReport, AppError> {
    let op = "run_disk_selftest";
    let qemu_img = crate::qemu::find_qemu_img()
        .ok_or_else(|| AppError::new("QEMU_IMG_MISSING", "qemu-img was not found", op))?;
    let dir = app_data(&app, op)?
        .join("selftest")
        .join(uuid::Uuid::new_v4().to_string());

    let joined = tauri::async_runtime::spawn_blocking(move || {
        disk_manager::self_test(Path::new(&qemu_img.path), &dir)
    })
    .await
    .map_err(|e| AppError::new("TASK_FAILED", e.to_string(), op))?;

    if let Ok(conn) = db.0.lock() {
        let (status, msg) = match &joined {
            Ok(_) => ("ok".to_string(), "Disk self-test passed".to_string()),
            Err(e) => ("error".to_string(), e.message.clone()),
        };
        let _ = database::log_activity(&conn, "disk_self_test", None, &status, &msg);
    }
    joined
}