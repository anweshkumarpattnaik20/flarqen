# FLARQEN release notes

## 0.2.0 - November 2026 (development release)

### New

- **Renamed to FLARQEN.** New logo, window title, icons and installer icons.
- **Templates.** Five presets when creating a sandbox: Blank, Quick throwaway, Try something risky, Software testing and Heavy workload. CPU and memory are limited to what your PC has.
- **Auto-delete timers.** Choose 30 minutes, 1 hour, 2 hours, 8 hours, 24 hours or 7 days. A countdown shows on the sandbox card and can be cancelled. Timers only run while the app is open. Anything overdue is deleted the next time the app starts.
- **Snapshots.** Save up to 5 named restore points per sandbox, restore to one, or delete one. The sandbox must be stopped.
- **Duplicate.** Copy a stopped sandbox. Snapshots are not copied.
- **Ctrl+K command bar.** Start or stop a sandbox, go to any page, run the setup check or change the theme.
- **Welcome checklist and Setup check.** Shows whether QEMU, hardware acceleration (WHPX) and a verified image are ready.
- **Splash screen, light and dark themes, page animations** and a friendlier empty state.

### Fixed

- A database repair migration creates the auto-delete and snapshot tables if an earlier development build left them missing. Without it, setting a timer failed with "no such table: sandbox_expiry".

### Changed

- 77 automated backend tests (up from 66).
- Database now has four migrations. Upgrading from 0.1.1 keeps all sandboxes and images.

### Tested

- MSI installer, and upgrade from 0.1.1 to 0.2.0 over the existing install, with all data kept.
- Setup .exe installer as a fresh install after an uninstall, with data kept.
- npm audit: 0 vulnerabilities. cargo audit: 0 vulnerabilities, 2 warnings in Linux-only or build-time packages that are not part of the Windows build.

### Known limits

- Installers are unsigned, so Windows SmartScreen shows a warning (More info, then Run anyway).
- QEMU is not bundled. Install it yourself.
- The shutdown channel listens on localhost without a password, so other programs on the same PC can connect to it.
- Not yet tested on a clean second PC, after an interrupted install, or with an .exe-over-.exe upgrade.
- Per-sandbox CPU percentage, custom storage location, export/import and safe file drop are not built.
- This is a development release and not production-ready.

## 0.1.1

- Installer worked; uninstall left user data in place; reinstall showed existing sandboxes and images.
- App data moved to `%LOCALAPPDATA%\DisposableOSSandbox`.

## 0.1.0

- First build: master image preparation, sandbox create/start/stop/reset/delete, resource monitor, activity log and settings.