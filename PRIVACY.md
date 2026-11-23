# FLARQEN privacy notice

Version 0.2.0, November 2026

**FLARQEN does not collect, send or share any data.**

- The app has no analytics, telemetry, crash reporting, update checks or accounts.
- The app makes no connections to the internet. Its only network use is a connection on your own PC (localhost) to the QEMU process it starts, so it can shut a virtual machine down cleanly.
- Virtual machines are started with networking turned off, so software inside a sandbox cannot reach the internet or your network.

## What is stored, and where

Everything stays on your PC in `%LOCALAPPDATA%\DisposableOSSandbox`:

- `sandbox.db`: your images, sandboxes, timers, snapshots and activity log
- `settings.json`: your settings
- `images` and `sandboxes`: master images, sandbox disks and snapshots

The files are not encrypted. Anyone who can use your Windows account can read them. Uninstalling the app leaves this folder in place. Delete it by hand to remove everything permanently.

## Other software

FLARQEN starts QEMU, which you install separately. QEMU is not covered by this notice. Operating systems and programs you run inside a sandbox have their own behaviour. FLARQEN turns the network off for the virtual machine, but if you turn it on in a guest by other means, that software can send data.

## Changes

If a later version adds any network feature, this notice and the release notes will say so first.