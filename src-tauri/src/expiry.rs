//! Auto-delete timer: a sandbox can be given a time after which it is shut down and deleted.
use crate::commands::DbState;
use crate::database;
use crate::errors::AppError;
use crate::lifecycle;
use crate::models::SandboxStatus;
use crate::sandbox_manager::{self, get_sandbox, lock, VmState};
use rusqlite::{params, Connection, OptionalExtension};
use std::time::Duration;
use tauri::{Manager, State};

const MIN_MINUTES: i64 = 5;
const MAX_MINUTES: i64 = 60 * 24 * 30;
/// After a failed attempt the timer is pushed back this long, so a stuck VM is not retried every sweep.
const RETRY_SECS: i64 = 300;
const SWEEP_EVERY_SECS: u64 = 30;

fn err(code: &str, msg: impl Into<String>, op: &str) -> AppError {
    AppError::new(code, msg, op)
}

/// Ids whose timer has run out, oldest first.
fn due(conn: &Connection, now: i64) -> Result<Vec<String>, AppError> {
    let mut st = conn.prepare("SELECT sandbox_id FROM sandbox_expiry WHERE expires_at <= ?1 ORDER BY expires_at")?;
    let rows = st.query_map([now], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn postpone(conn: &Connection, id: &str, now: i64) {
    let _ = conn.execute(
        "UPDATE sandbox_expiry SET expires_at = ?1 WHERE sandbox_id = ?2",
        params![now + RETRY_SECS, id],
    );
}

/// Sets the timer to `minutes` from now, or clears it when `minutes` is None. Returns the end time.
#[tauri::command]
pub async fn set_sandbox_expiry(
    db: State<'_, DbState>,
    id: String,
    minutes: Option<i64>,
) -> Result<Option<i64>, AppError> {
    let op = "set_sandbox_expiry";
    let conn = lock(&db, op)?;
    let sb = get_sandbox(&conn, &id)?;
    match minutes {
        None => {
            conn.execute("DELETE FROM sandbox_expiry WHERE sandbox_id = ?1", [&id])?;
            database::log_activity(&conn, "sandbox_timer", Some(&id), "ok", &format!("Auto-delete timer removed from '{}'", sb.name))?;
            Ok(None)
        }
        Some(m) => {
            if !(MIN_MINUTES..=MAX_MINUTES).contains(&m) {
                return Err(err("TIMER_RANGE", "The timer must be between 5 minutes and 30 days", op));
            }
            let at = database::now() + m * 60;
            conn.execute(
                "INSERT INTO sandbox_expiry (sandbox_id, expires_at) VALUES (?1, ?2)
                 ON CONFLICT(sandbox_id) DO UPDATE SET expires_at = excluded.expires_at",
                params![id, at],
            )?;
            database::log_activity(
                &conn,
                "sandbox_timer",
                Some(&id),
                "ok",
                &format!("'{}' will be deleted automatically in {} minutes", sb.name, m),
            )?;
            Ok(Some(at))
        }
    }
}

#[tauri::command]
pub async fn get_sandbox_expiry(db: State<'_, DbState>, id: String) -> Result<Option<i64>, AppError> {
    let conn = lock(&db, "get_sandbox_expiry")?;
    Ok(conn
        .query_row("SELECT expires_at FROM sandbox_expiry WHERE sandbox_id = ?1", [&id], |r| r.get::<_, i64>(0))
        .optional()?)
}

/// Shuts the sandbox down if needed, then deletes it with the same code as the Delete button.
async fn destroy_one(app: &tauri::AppHandle, id: &str) -> Result<String, AppError> {
    let status = {
        let db: State<DbState> = app.state();
        let vm: State<VmState> = app.state();
        let conn = lock(&db, "auto_delete")?;
        sandbox_manager::reconcile(&conn, &vm)?;
        get_sandbox(&conn, id)?.status
    };
    match status {
        SandboxStatus::Stopped | SandboxStatus::Error => {}
        SandboxStatus::Running | SandboxStatus::Stopping => {
            let graceful =
                sandbox_manager::stop_sandbox(app.clone(), app.state(), app.state(), id.to_string(), Some(false)).await;
            if graceful.is_err() {
                // The time is up: cut the power rather than leave it running.
                sandbox_manager::stop_sandbox(app.clone(), app.state(), app.state(), id.to_string(), Some(true)).await?;
            }
        }
        other => {
            return Err(err("BUSY", format!("Sandbox is {}; will try again later", other.as_str()), "auto_delete"));
        }
    }
    lifecycle::delete_sandbox(app.clone(), app.state(), app.state(), id.to_string()).await
}

async fn sweep(app: &tauri::AppHandle) {
    let db: State<DbState> = app.state();
    let ids = match db.0.lock() {
        Ok(conn) => due(&conn, database::now()).unwrap_or_default(),
        Err(_) => return,
    };
    for id in ids {
        match destroy_one(app, &id).await {
            Ok(_) => {
                if let Ok(conn) = db.0.lock() {
                    let _ = database::log_activity(&conn, "sandbox_expire", Some(&id), "ok", "Auto-delete timer ran out; sandbox deleted");
                }
            }
            Err(e) => {
                if let Ok(conn) = db.0.lock() {
                    postpone(&conn, &id, database::now());
                    let _ = database::log_activity(
                        &conn,
                        "sandbox_expire",
                        Some(&id),
                        "error",
                        &format!("Auto-delete failed, will retry in 5 minutes: {}", e.message),
                    );
                }
            }
        }
    }
}

/// Starts the background checker. It runs only while the app is open.
pub fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(10));
        loop {
            tauri::async_runtime::block_on(sweep(&app));
            std::thread::sleep(Duration::from_secs(SWEEP_EVERY_SECS));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{OsImage, Sandbox};

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

    #[test]
    fn only_overdue_timers_are_due_oldest_first() {
        let c = db_with(&["a", "b", "c"]);
        c.execute("INSERT INTO sandbox_expiry VALUES ('a', 300), ('b', 100), ('c', 9000)", []).unwrap();
        assert_eq!(due(&c, 500).unwrap(), vec!["b".to_string(), "a".to_string()]);
        assert!(due(&c, 50).unwrap().is_empty());
    }

    #[test]
    fn timer_row_is_removed_with_its_sandbox() {
        let c = db_with(&["a", "b"]);
        c.execute("INSERT INTO sandbox_expiry VALUES ('a', 100), ('b', 100)", []).unwrap();
        c.execute("DELETE FROM sandboxes WHERE id = 'a'", []).unwrap();
        assert_eq!(due(&c, 1000).unwrap(), vec!["b".to_string()]);
    }

    #[test]
    fn postpone_moves_the_timer_forward() {
        let c = db_with(&["a"]);
        c.execute("INSERT INTO sandbox_expiry VALUES ('a', 100)", []).unwrap();
        postpone(&c, "a", 1000);
        assert!(due(&c, 1000).unwrap().is_empty());
        assert_eq!(due(&c, 1000 + RETRY_SECS).unwrap().len(), 1);
    }

    #[test]
    fn timer_for_a_missing_sandbox_is_rejected() {
        let c = db_with(&[]);
        assert!(c.execute("INSERT INTO sandbox_expiry VALUES ('ghost', 1)", []).is_err());
    }
}