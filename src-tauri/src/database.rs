#![allow(dead_code)]
use crate::errors::AppError;
use crate::models::{ActivityEntry, OsImage, Sandbox, SandboxStatus};
use rusqlite::{params, Connection};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

// Append new migrations to this list. Never edit an existing one.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/001_init.sql"),
    include_str!("../migrations/002_expiry.sql"),
    include_str!("../migrations/003_snapshots.sql"),
    include_str!("../migrations/004_repair.sql"),
];

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn open(path: &Path) -> Result<Connection, AppError> {
    init(Connection::open(path)?)
}

pub fn open_in_memory() -> Result<Connection, AppError> {
    init(Connection::open_in_memory()?)
}

fn init(mut conn: Connection) -> Result<Connection, AppError> {
    conn.pragma_update(None, "foreign_keys", true)?;
    migrate(&mut conn)?;
    Ok(conn)
}

pub fn schema_version(conn: &Connection) -> Result<i64, AppError> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

fn migrate(conn: &mut Connection) -> Result<(), AppError> {
    let current = schema_version(conn)?;
    let target = MIGRATIONS.len() as i64;
    if current > target {
        return Err(AppError::new(
            "DB_TOO_NEW",
            "The database was created by a newer version of this app",
            "migrate",
        ));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

pub fn insert_image(conn: &Connection, img: &OsImage) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO os_images (id,name,version,image_path,image_format,checksum,verified,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![img.id, img.name, img.version, img.image_path, img.image_format, img.checksum, img.verified, img.created_at],
    )?;
    Ok(())
}

pub fn insert_sandbox(conn: &Connection, s: &Sandbox) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO sandboxes (id,name,description,operating_system,os_version,base_image_id,overlay_path,
         cpu_cores,ram_mb,disk_capacity_gb,status,process_id,created_at,updated_at,last_started_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        params![
            s.id, s.name, s.description, s.operating_system, s.os_version, s.base_image_id,
            s.overlay_path, s.cpu_cores, s.ram_mb, s.disk_capacity_gb, s.status.as_str(),
            s.process_id, s.created_at, s.updated_at, s.last_started_at
        ],
    )?;
    Ok(())
}

fn row_to_sandbox(r: &rusqlite::Row) -> rusqlite::Result<Sandbox> {
    Ok(Sandbox {
        id: r.get(0)?,
        name: r.get(1)?,
        description: r.get(2)?,
        operating_system: r.get(3)?,
        os_version: r.get(4)?,
        base_image_id: r.get(5)?,
        overlay_path: r.get(6)?,
        cpu_cores: r.get(7)?,
        ram_mb: r.get(8)?,
        disk_capacity_gb: r.get(9)?,
        status: SandboxStatus::parse(&r.get::<_, String>(10)?),
        process_id: r.get(11)?,
        created_at: r.get(12)?,
        updated_at: r.get(13)?,
        last_started_at: r.get(14)?,
    })
}

pub fn list_sandboxes(conn: &Connection) -> Result<Vec<Sandbox>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id,name,description,operating_system,os_version,base_image_id,overlay_path,
         cpu_cores,ram_mb,disk_capacity_gb,status,process_id,created_at,updated_at,last_started_at
         FROM sandboxes ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], row_to_sandbox)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Changes status only along an allowed transition.
pub fn update_status(conn: &Connection, id: &str, next: SandboxStatus, pid: Option<u32>) -> Result<(), AppError> {
    let cur: String = conn
        .query_row("SELECT status FROM sandboxes WHERE id = ?1", [id], |r| r.get(0))
        .map_err(|_| AppError::new("NOT_FOUND", "Sandbox not found", "update_status"))?;
    let cur = SandboxStatus::parse(&cur);
    if cur != next && !cur.can_transition_to(next) {
        return Err(AppError::new(
            "INVALID_TRANSITION",
            format!("Cannot change status from {} to {}", cur.as_str(), next.as_str()),
            "update_status",
        ));
    }
    let t = now();
    let started = if next == SandboxStatus::Running { Some(t) } else { None };
    conn.execute(
        "UPDATE sandboxes SET status=?1, process_id=?2, updated_at=?3,
         last_started_at=COALESCE(?4,last_started_at) WHERE id=?5",
        params![next.as_str(), pid, t, started, id],
    )?;
    Ok(())
}

pub fn log_activity(conn: &Connection, operation: &str, sandbox_id: Option<&str>, status: &str, message: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO activity_log (operation,sandbox_id,status,message,timestamp) VALUES (?1,?2,?3,?4,?5)",
        params![operation, sandbox_id, status, message, now()],
    )?;
    Ok(())
}

pub fn list_activity(conn: &Connection, limit: i64) -> Result<Vec<ActivityEntry>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id,operation,sandbox_id,status,message,timestamp FROM activity_log ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit.clamp(1, 500)], |r| {
        Ok(ActivityEntry {
            id: r.get(0)?,
            operation: r.get(1)?,
            sandbox_id: r.get(2)?,
            status: r.get(3)?,
            message: r.get(4)?,
            timestamp: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> OsImage {
        OsImage {
            id: "img1".into(), name: "Ubuntu".into(), version: "24.04".into(),
            image_path: "C:/images/ubuntu.qcow2".into(), image_format: "qcow2".into(),
            checksum: None, verified: true, created_at: 1,
        }
    }

    fn sandbox(id: &str, name: &str) -> Sandbox {
        Sandbox {
            id: id.into(), name: name.into(), description: String::new(),
            operating_system: "Ubuntu".into(), os_version: "24.04".into(),
            base_image_id: "img1".into(), overlay_path: format!("{id}/overlay.qcow2"),
            cpu_cores: 2, ram_mb: 2048, disk_capacity_gb: 20,
            status: SandboxStatus::Stopped, process_id: None,
            created_at: 1, updated_at: 1, last_started_at: None,
        }
    }

    #[test]
    fn migrations_apply_and_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let c1 = open(&path).unwrap();
        assert_eq!(schema_version(&c1).unwrap(), MIGRATIONS.len() as i64);
        let n: i64 = c1
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('os_images','sandboxes','activity_log')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 3, "all three tables must exist after migration");
        drop(c1);
        let c2 = open(&path).unwrap(); // reopening must not re-run or fail
        assert_eq!(schema_version(&c2).unwrap(), MIGRATIONS.len() as i64);
    }

    #[test]
    fn duplicate_names_are_rejected_case_insensitively() {
        let c = open_in_memory().unwrap();
        insert_image(&c, &image()).unwrap();
        insert_sandbox(&c, &sandbox("a", "Lab")).unwrap();
        assert!(insert_sandbox(&c, &sandbox("b", "lab")).is_err());
    }

    #[test]
    fn sandbox_requires_an_existing_image() {
        let c = open_in_memory().unwrap();
        assert!(insert_sandbox(&c, &sandbox("a", "Lab")).is_err()); // foreign key
    }

    #[test]
    fn status_updates_follow_transition_rules() {
        let c = open_in_memory().unwrap();
        insert_image(&c, &image()).unwrap();
        insert_sandbox(&c, &sandbox("a", "Lab")).unwrap();
        assert!(update_status(&c, "a", SandboxStatus::Running, Some(1)).is_err()); // skips Starting
        update_status(&c, "a", SandboxStatus::Starting, None).unwrap();
        update_status(&c, "a", SandboxStatus::Running, Some(4242)).unwrap();
        let s = &list_sandboxes(&c).unwrap()[0];
        assert_eq!(s.status, SandboxStatus::Running);
        assert_eq!(s.process_id, Some(4242));
        assert!(s.last_started_at.is_some());
    }

    #[test]
    fn activity_log_records_and_lists_newest_first() {
        let c = open_in_memory().unwrap();
        log_activity(&c, "first", None, "ok", "one").unwrap();
        log_activity(&c, "second", None, "ok", "two").unwrap();
        let l = list_activity(&c, 10).unwrap();
        assert_eq!(l[0].operation, "second");
        assert_eq!(l.len(), 2);
    }
}