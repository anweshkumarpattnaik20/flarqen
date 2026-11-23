# Third-party notices

FLARQEN is built with the open-source components below. Each keeps its own
licence and copyright. This is a summary, not a complete list. Check each
package's own licence for the exact terms.

## Used at run time, not bundled

- **QEMU** (GPL-2.0 and other licences). FLARQEN starts QEMU as a separate
  program that you install yourself. FLARQEN does not include, copy or modify
  QEMU. Source and licence: https://www.qemu.org

## Application framework

- **Tauri** (MIT or Apache-2.0) - desktop app framework
- **Rust standard library and crates**, including serde, uuid, tempfile and
  sysinfo (mostly MIT or Apache-2.0)
- **rusqlite** (MIT), with **SQLite** (public domain)

## Front end

- **React** (MIT)
- **Vite** (MIT) and **TypeScript** (Apache-2.0) - build tools
- **Tailwind CSS** (MIT)
- **lucide-react** (ISC) - icons

## Windows components used by the installers

- **WiX Toolset** (MS-RL) and **NSIS** (zlib/libpng licence) - used by Tauri to
  build the installers

## FLARQEN name and logo

The FLARQEN name and logo belong to the copyright holder named in LICENSE.txt.

## Full lists

To produce a complete, exact list for a release, run:

    npm ls --all
    cargo install cargo-license
    cargo license