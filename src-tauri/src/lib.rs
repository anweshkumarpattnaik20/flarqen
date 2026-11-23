mod commands;
mod database;
mod disk_manager;
mod duplicate;
mod errors;
mod expiry;
mod host;
mod installer;
mod lifecycle;
mod models;
mod monitor;
mod qemu;
mod sandbox_manager;
mod settings;
mod snapshots;
mod validation;

use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // One data folder for everything (database, settings, images, sandbox disks).
            let dir = sandbox_manager::app_data(app.handle(), "startup")?;
            let conn = database::open(&dir.join("sandbox.db"))?;
            database::log_activity(&conn, "app_start", None, "ok", "Application started")?;

            // Settings first, so custom QEMU locations are in effect before anything looks for QEMU.
            let settings_file = dir.join("settings.json");
            let (loaded, warning) = settings::load_or_default(&settings_file);
            settings::apply(&loaded);
            if let Some(w) = warning {
                database::log_activity(&conn, "settings_load", None, "warning", &w)?;
            }
            let _ = settings::prune_activity(&conn, loaded.log_retention_days);

            let vm = sandbox_manager::VmState::new();
            // Fix up anything left over from a crash or a previous run.
            lifecycle::recover_interrupted(&conn)?;
            sandbox_manager::reconcile(&conn, &vm)?;
            let sys = sysinfo::System::new()
                .map_err(|e| format!("could not read host information: {e:?}"))?;
            app.manage(commands::DbState(Mutex::new(conn)));
            app.manage(installer::InstallState::new());
            app.manage(monitor::MonitorState(Mutex::new(sys)));
            app.manage(settings::SettingsState {
                current: Mutex::new(loaded),
                file: settings_file,
            });
            app.manage(vm);
            expiry::start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            qemu::detect_qemu,
            commands::get_sandbox_list,
            commands::get_activity_log,
            commands::run_disk_selftest,
            installer::start_master_install,
            installer::get_install_status,
            installer::finish_master_install,
            installer::cancel_master_install,
            installer::list_os_images,
            sandbox_manager::list_sandboxes,
            sandbox_manager::create_sandbox,
            sandbox_manager::start_sandbox,
            sandbox_manager::stop_sandbox,
            sandbox_manager::get_vm_log,
            sandbox_manager::verify_image,
            duplicate::duplicate_sandbox,
            expiry::set_sandbox_expiry,
            expiry::get_sandbox_expiry,
            snapshots::list_snapshots,
            snapshots::create_snapshot,
            snapshots::restore_snapshot,
            snapshots::delete_snapshot,
            lifecycle::reset_sandbox,
            lifecycle::delete_sandbox,
            monitor::get_host_metrics,
            monitor::get_storage_info,
            monitor::get_sandbox_metrics,
            settings::get_settings,
            settings::save_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}