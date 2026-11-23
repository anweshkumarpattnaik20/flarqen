# FLARQEN

### Disposable OS Sandbox Manager

**Create. Isolate. Test. Destroy.**

FLARQEN is a desktop application designed to manage disposable operating system sandboxes in an isolated environment. It provides a centralized interface for creating, managing, monitoring, and maintaining sandbox instances.

Built with modern desktop technologies, FLARQEN aims to make isolated computing environments easier to manage while providing users with control over sandbox lifecycles, snapshots, and system resources.

---

## ✨ Overview

FLARQEN is designed for developers, security researchers, testers, and technical users who need temporary and isolated operating system environments.

Instead of configuring and managing everything manually, FLARQEN brings essential sandbox management operations into one desktop interface.

## 🚀 Features

- **Sandbox Management** – Create, manage, and organize disposable sandbox environments.
- **Snapshot Management** – Manage sandbox snapshots and restore previous states.
- **Lifecycle Management** – Control sandbox operations and environment lifecycles.
- **System Monitoring** – Monitor sandbox activity and resource usage.
- **Image Management** – Manage operating system images used by sandbox environments.
- **Activity Logs** – Access application and sandbox-related logs.
- **Expiry Management** – Support temporary sandbox lifetimes and expiry operations.
- **Settings & Configuration** – Configure application preferences and sandbox settings.
- **Desktop Experience** – Native desktop application powered by Tauri.

> Note: Actual feature availability depends on the configured virtualization backend and operating system.

---

## 🛠️ Technology Stack

| Technology | Purpose |
|---|---|
| React.js | Frontend UI |
| TypeScript | Type safety |
| Vite | Frontend development and bundling |
| Tailwind CSS | UI styling |
| Tauri | Desktop application framework |
| Rust | Backend functionality |
| SQLite | Local database |
| QEMU | Virtualization backend |

---

## 📁 Project Structure

```text
FLARQEN/
│
├── assets/
├── public/
│
├── src/
│   ├── components/
│   ├── pages/
│   ├── App.tsx
│   ├── App.css
│   ├── api.ts
│   ├── context.tsx
│   ├── settings.ts
│   └── types.ts
│
├── src-tauri/
│   ├── migrations/
│   ├── src/
│   │   ├── commands.rs
│   │   ├── database.rs
│   │   ├── disk_manager.rs
│   │   ├── expiry.rs
│   │   ├── host.rs
│   │   ├── installer.rs
│   │   ├── lifecycle.rs
│   │   ├── monitor.rs
│   │   ├── qemu.rs
│   │   ├── sandbox_manager.rs
│   │   └── snapshots.rs
│   │
│   ├── Cargo.toml
│   └── tauri.conf.json
│
├── package.json
├── package-lock.json
├── vite.config.ts
├── tsconfig.json
└── README.md
```

---

## 💻 Getting Started

### Prerequisites

Before installing FLARQEN, make sure you have the following tools installed:

- Node.js (LTS recommended)
- npm
- Rust
- Cargo
- Microsoft C++ Build Tools (Windows)
- WebView2 Runtime (Windows)
- Tauri system dependencies

If you intend to use QEMU virtualization, install and configure a compatible QEMU environment separately.

### 1. Clone the repository

```bash
git clone https://github.com/anweshkumarpattnaik20/flarqen.git
```

### 2. Navigate to the project

```bash
cd flarqen
```

### 3. Install dependencies

```bash
npm install
```

### 4. Start the development server

```bash
npm run tauri dev
```

This starts the frontend development environment and launches the Tauri desktop application.

---

## 🏗️ Production Build

To create a production desktop application:

```bash
npm run tauri build
```

The generated application bundles will be available in the Tauri build output directory, depending on your target platform and configuration.

---

## 🔧 Development Commands

| Command | Description |
|---|---|
| `npm install` | Install dependencies |
| `npm run dev` | Start frontend development server |
| `npm run build` | Build frontend |
| `npm run tauri dev` | Run desktop application |
| `npm run tauri build` | Build desktop application |

---

## 🔐 Security & Isolation

FLARQEN is designed around the concept of disposable computing environments.

Its architecture includes components for:

- Sandbox lifecycle management.
- Virtual machine management.
- Disk and storage management.
- Snapshot operations.
- Environment expiry.
- Database-backed configuration.

**Important:** A disposable sandbox should not automatically be considered a security boundary. Actual isolation depends on the virtualization configuration, host security, permissions, and backend implementation. Do not use an unverified configuration to execute untrusted code.

---

## 🗺️ Roadmap

The following are potential development priorities:

- [ ] Improve sandbox creation workflows.
- [ ] Expand virtualization backend support.
- [ ] Enhance resource monitoring.
- [ ] Improve snapshot and recovery workflows.
- [ ] Strengthen sandbox isolation and security.
- [ ] Improve error handling and diagnostics.
- [ ] Add comprehensive automated testing.
- [ ] Improve cross-platform compatibility.

---

## 🤝 Contributing

Contributions, bug reports, and feature suggestions are welcome.

To contribute:

1. Fork the repository.
2. Create a feature branch.
3. Implement your changes.
4. Test your changes.
5. Submit a Pull Request.

```bash
git checkout -b feature/your-feature-name
git add .
git commit -m "Add: your feature description"
git push origin feature/your-feature-name
```

---

## 📄 License

Please refer to [LICENSE.txt](LICENSE.txt) for licensing information.

---

## 👨‍💻 Author

**Anwesh Kumar Pattnaik**

GitHub: [@anweshkumarpattnaik20](https://github.com/anweshkumarpattnaik20)

Project: [FLARQEN](https://github.com/anweshkumarpattnaik20/flarqen)

---

<p align="center">

**FLARQEN**

*Your environment. Your control. Your isolation.*

</p>
