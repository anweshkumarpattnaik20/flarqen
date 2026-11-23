# FLARQEN - Disposable OS Sandbox

FLARQEN is a Windows 11 desktop app that gives you disposable virtual machines. You install an operating system once as a protected master image, then create as many throwaway sandboxes from it as you like. Reset or delete a sandbox at any time and the master stays untouched.

Status: **development release 0.2.0. Not production-ready.** See "Known limits" below.

## How it works

- The master image is a QCOW2 file. It is read-only once registered and its SHA-256 checksum is verified.
- Every sandbox gets its own copy-on-write overlay that points back to the master. Guests write only to their overlay.
- Virtual machines run with QEMU and Windows Hypervisor Platform (WHPX) acceleration, with networking turned off.
- Reset throws the overlay away. Delete removes one sandbox's folder only.

## Features

- Five sandbox templates, with optional auto-delete timers (30 minutes to 7 days)
- Snapshots (up to 5 per sandbox), duplicate, reset, force stop
- Resource monitor, activity log, setup check, Ctrl+K command bar
- Light and dark themes

## Requirements (to use the app)

- Windows 11, 64-bit, with virtualization enabled in the BIOS/UEFI
- Windows Hypervisor Platform turned on (then restart)
- QEMU for Windows installed separately (FLARQEN does not bundle it)
- About 25 GB free disk space to create a sandbox, plus a safety reserve

## Requirements (to build the app)

- Node.js and npm
- Rust (stable) with the MSVC toolchain and Visual Studio Build Tools (C++ workload)
- Microsoft Edge WebView2 runtime (included with Windows 11)

## Build and run

```powershell
npm install
npm run tauri dev      # run in development
npm run tauri build    # build the installers
```

Installers are written to `src-tauri\target\release\bundle`:

- `msi\disposable-os-sandbox_<version>_x64_en-US.msi`
- `nsis\disposable-os-sandbox_<version>_x64-setup.exe`

The installers are unsigned, so Windows SmartScreen shows a warning. Click More info, then Run anyway.

## Tests and checks

```powershell
cd src-tauri
cargo test             # 77 backend tests
cargo audit            # known vulnerabilities in Rust dependencies
cd ..
npm audit              # known vulnerabilities in npm dependencies
```

## Project layout

- `src` - React, TypeScript and Tailwind front end
- `src-tauri/src` - Rust backend: database, sandbox lifecycle, QEMU control, timers, snapshots
- `src-tauri/migrations` - SQLite migrations. Add new ones to the list in `database.rs`. Never edit an old migration.
- `src-tauri/tauri.conf.json` - app name, version, window and security settings
- `public` - logos

## Where your data lives

`%LOCALAPPDATA%\DisposableOSSandbox` holds `sandbox.db`, `settings.json`, `images` and `sandboxes`. Uninstalling the app leaves this folder in place. Delete it by hand to remove everything permanently.

## Security notes

- No guest network, shared folders, clipboard sharing or USB passthrough.
- The shutdown channel listens on localhost only. It has no password, so other programs on the same PC can connect to it.
- Virtualization does not guarantee isolation. This is not a replacement for a dedicated malware-analysis lab.

## Known limits

- Installers are unsigned. QEMU is not bundled.
- Auto-delete timers only run while the app is open.
- Duplicates do not include snapshots.
- Not yet tested on a clean second PC, after an interrupted install, or with an .exe-over-.exe upgrade.
- Export, import and safe file drop are not implemented.

## Licence

Not yet chosen. QEMU and the other third-party components keep their own licences.