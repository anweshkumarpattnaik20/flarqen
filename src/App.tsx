import "./App.css";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Activity, Bell, FileText, HardDrive, Info, LayoutDashboard, ListChecks, Plus, Search, Server, Settings as SettingsIcon } from "lucide-react";
import { api, errText } from "./api";
import { AppContext } from "./context";
import type { Accel, AppCtx, ConfirmOpts, Theme } from "./context";
import { DEFAULT_SETTINGS } from "./settings";
import type { Settings } from "./settings";
import type { OsImage, Page, QemuDetection, Sandbox } from "./types";
import CreateWizard from "./pages/CreateWizard";
import Dashboard from "./pages/Dashboard";
import Help from "./pages/Help";
import Images from "./pages/Images";
import Logs from "./pages/Logs";
import Monitor from "./pages/Monitor";
import Sandboxes from "./pages/Sandboxes";
import SettingsPage from "./pages/Settings";
import CommandBar from "./components/CommandBar";
import Splash from "./components/Splash";
import Welcome from "./components/Welcome";

type Note = { id: number; kind: "success" | "error" | "info"; text: string; time: number };

const NAV: { id: Page; label: string; icon: typeof LayoutDashboard }[] = [
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "sandboxes", label: "My Sandboxes", icon: Server },
  { id: "create", label: "Create Sandbox", icon: Plus },
  { id: "images", label: "OS Images", icon: HardDrive },
  { id: "monitor", label: "Resource Monitor", icon: Activity },
  { id: "logs", label: "Activity Logs", icon: FileText },
  { id: "settings", label: "Settings", icon: SettingsIcon },
  { id: "help", label: "Help", icon: Info },
];

function load<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    if (v && (allowed as readonly string[]).includes(v)) return v as T;
  } catch {
    /* storage unavailable */
  }
  return fallback;
}

function save(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* storage unavailable */
  }
}

export default function App() {
  const [page, setPage] = useState<Page>("dashboard");
  const [images, setImages] = useState<OsImage[]>([]);
  const [sandboxes, setSandboxes] = useState<Sandbox[]>([]);
  const [qemu, setQemu] = useState<QemuDetection | null>(null);
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [backendErr, setBackendErr] = useState<string | null>(null);
  const [busyIds, setBusyIds] = useState<string[]>([]);
  const [confirm, setConfirm] = useState<ConfirmOpts | null>(null);
  const [notes, setNotes] = useState<Note[]>([]);
  const [toasts, setToasts] = useState<Note[]>([]);
  const [bellOpen, setBellOpen] = useState(false);
  const [theme, setThemeState] = useState<Theme>(() => load("dos-theme", ["system", "light", "dark"] as const, "system"));
  const [welcome, setWelcome] = useState<boolean>(() => {
    try {
      return localStorage.getItem("dos-welcome-done") !== "1";
    } catch {
      return true;
    }
  });
  const [splash, setSplash] = useState(true);
  const [palette, setPalette] = useState(false);
  const nid = useRef(0);

  const accel: Accel = settings.acceleration === "tcg" ? "tcg" : "whpx";

  const setTheme = useCallback((t: Theme) => {
    setThemeState(t);
    save("dos-theme", t);
  }, []);

  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = theme === "dark" || (theme === "system" && mq.matches);
      document.documentElement.classList.toggle("dark", dark);
    };
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((p) => !p);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const endSplash = useCallback(() => setSplash(false), []);
  const openSetup = useCallback(() => setWelcome(true), []);
  const closePalette = useCallback(() => setPalette(false), []);

  const closeWelcome = useCallback(() => {
    setWelcome(false);
    save("dos-welcome-done", "1");
  }, []);

  const notify = useCallback((kind: Note["kind"], text: string) => {
    const n: Note = { id: ++nid.current, kind, text, time: Date.now() };
    setNotes((p) => [n, ...p].slice(0, 30));
    setToasts((p) => [...p, n]);
    setTimeout(() => setToasts((p) => p.filter((t) => t.id !== n.id)), 6000);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const [imgs, sbs] = await Promise.all([api.images(), api.sandboxes()]);
      setImages(imgs);
      setSandboxes(sbs);
      setBackendErr(null);
    } catch (e) {
      setBackendErr(errText(e));
    }
  }, []);

  const refreshQemu = useCallback(async () => {
    try {
      setQemu(await api.qemu());
    } catch (e) {
      setBackendErr(errText(e));
    }
  }, []);

  useEffect(() => {
    api
      .settings()
      .then(setSettings)
      .catch((e) => setBackendErr(errText(e)));
    void refresh();
    void refreshQemu();
    // The backend corrects each sandbox's status from the real process on every call.
    const t = setInterval(() => {
      if (!document.hidden) void refresh();
    }, 3000);
    return () => clearInterval(t);
  }, [refresh, refreshQemu]);

  const run = useCallback(
    async <T,>(label: string, fn: () => Promise<T>, id = "global"): Promise<T | undefined> => {
      setBusyIds((p) => [...p, id]);
      try {
        const r = await fn();
        notify("success", `${label}: done`);
        return r;
      } catch (e) {
        notify("error", `${label} failed: ${errText(e)}`);
        return undefined;
      } finally {
        setBusyIds((p) => {
          const i = p.indexOf(id);
          if (i < 0) return p;
          const c = [...p];
          c.splice(i, 1);
          return c;
        });
        void refresh();
      }
    },
    [notify, refresh],
  );

  const saveSettings = useCallback(
    async (s: Settings): Promise<boolean> => {
      const saved = await run("Save settings", () => api.saveSettings(s));
      if (saved) {
        setSettings(saved);
        await refreshQemu();
        return true;
      }
      return false;
    },
    [run, refreshQemu],
  );

  const ctx: AppCtx = useMemo(
    () => ({
      images,
      sandboxes,
      qemu,
      settings,
      refresh,
      refreshQemu,
      saveSettings,
      run,
      busyIds,
      ask: setConfirm,
      go: setPage,
      notify,
      theme,
      setTheme,
      accel,
    }),
    [images, sandboxes, qemu, settings, refresh, refreshQemu, saveSettings, run, busyIds, notify, theme, setTheme, accel],
  );

  const running = sandboxes.filter((s) => s.status === "running").length;
  const virt = !qemu
    ? { text: "Checking...", cls: "bg-slate-500" }
    : !qemu.qemu
      ? { text: "QEMU not found", cls: "bg-red-600" }
      : qemu.whpx_listed
        ? { text: "WHPX available", cls: "bg-emerald-600" }
        : { text: "No WHPX (slow mode only)", cls: "bg-amber-600" };

  return (
    <AppContext.Provider value={ctx}>
      <div className="flex h-full">
        <aside className="app-card m-0 flex w-60 shrink-0 flex-col rounded-none border-y-0 border-l-0 p-3">
          <div className="mb-5 flex items-center gap-2.5 px-2 pt-1">
            <img src="/logo-mark.png" alt="" className="h-auto w-12 shrink-0" draggable={false} />
            <div className="min-w-0">
              <img src="/logo-wordmark-dark.png" alt="FLARQEN" className="block h-auto w-[132px] dark:hidden" draggable={false} />
              <img src="/logo-wordmark-light.png" alt="FLARQEN" className="hidden h-auto w-[132px] dark:block" draggable={false} />
              <div className="app-muted mt-1.5 whitespace-nowrap text-[8px] font-medium uppercase leading-tight tracking-[0.1em]">
                Disposable OS Sandbox
              </div>
            </div>
          </div>
          <nav className="flex flex-col gap-1">
            {NAV.map((n) => (
              <button
                key={n.id}
                className={`app-nav ${page === n.id ? "app-nav-active" : ""}`}
                onClick={() => setPage(n.id)}
              >
                <n.icon className="h-4 w-4" />
                {n.label}
              </button>
            ))}
          </nav>
          <div className="mt-auto">
            <button className="app-nav w-full" onClick={() => setWelcome(true)}>
              <ListChecks className="h-4 w-4" />
              Setup check
            </button>
            <div className="app-muted mt-2 px-2 text-xs">Development build - not production-ready</div>
          </div>
        </aside>

        <div className="flex min-w-0 flex-1 flex-col">
          <header className="app-card relative flex items-center justify-between rounded-none border-x-0 border-t-0 px-4 py-2">
            <div className="flex items-center gap-3 text-xs">
              <span className={`rounded px-2 py-0.5 font-medium text-white ${virt.cls}`} title="Virtualization status as reported by QEMU">
                {virt.text}
              </span>
              <span className="app-muted">{running} running</span>
            </div>
            <div className="flex items-center gap-2">
              <button
                className="app-btn app-btn-ghost flex items-center gap-2"
                aria-label="Open the command bar"
                title="Command bar (Ctrl+K)"
                onClick={() => setPalette(true)}
              >
                <Search className="h-4 w-4" />
                <span className="app-muted text-xs">Ctrl K</span>
              </button>
              <select
                className="app-input w-32"
                aria-label="Theme"
                value={theme}
                onChange={(e) => setTheme(e.target.value as Theme)}
              >
                <option value="system">System</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </select>
              <button className="app-btn app-btn-ghost" aria-label="Notifications" onClick={() => setBellOpen(!bellOpen)}>
                <Bell className="h-4 w-4" />
              </button>
              <button className="app-btn app-btn-ghost" aria-label="Settings" onClick={() => setPage("settings")}>
                <SettingsIcon className="h-4 w-4" />
              </button>
            </div>
            {bellOpen && (
              <div className="app-card absolute right-4 top-12 z-20 max-h-80 w-96 overflow-auto p-2 text-xs shadow-lg">
                {notes.length === 0 && <div className="app-muted p-2">No notifications this session.</div>}
                {notes.map((n) => (
                  <div key={n.id} className="border-b p-2" style={{ borderColor: "var(--border)" }}>
                    <span className={n.kind === "error" ? "text-red-500" : n.kind === "success" ? "text-emerald-500" : ""}>
                      {new Date(n.time).toLocaleTimeString()}
                    </span>{" "}
                    {n.text}
                  </div>
                ))}
              </div>
            )}
          </header>

          {backendErr && (
            <div className="bg-red-600 px-4 py-2 text-sm text-white">Backend problem: {backendErr}</div>
          )}

          <main key={page} className="page-enter flex-1 overflow-auto p-6">
            {page === "dashboard" && <Dashboard />}
            {page === "sandboxes" && <Sandboxes />}
            {page === "create" && <CreateWizard />}
            {page === "images" && <Images />}
            {page === "monitor" && <Monitor />}
            {page === "logs" && <Logs />}
            {page === "settings" && <SettingsPage />}
            {page === "help" && <Help />}
          </main>
        </div>
      </div>

      <div className="fixed bottom-4 right-4 z-30 flex w-96 flex-col gap-2">
        {toasts.map((t) => (
          <div
            key={t.id}
            className={`rounded-lg px-3 py-2 text-sm text-white shadow-lg ${
              t.kind === "error" ? "bg-red-600" : t.kind === "success" ? "bg-emerald-600" : "bg-slate-700"
            }`}
          >
            {t.text}
          </div>
        ))}
      </div>

      {welcome && <Welcome onClose={closeWelcome} />}
      {palette && <CommandBar onClose={closePalette} onSetup={openSetup} />}
      {splash && <Splash onDone={endSplash} />}

      {confirm && (
        <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/60" role="dialog" aria-modal="true">
          <div className="app-card pop-in w-96 p-5">
            <h3 className="text-lg font-semibold">{confirm.title}</h3>
            <p className="app-muted mt-2 text-sm">{confirm.body}</p>
            <div className="mt-4 flex justify-end gap-2">
              <button className="app-btn app-btn-ghost" onClick={() => setConfirm(null)}>
                Cancel
              </button>
              <button
                className="app-btn app-btn-danger"
                onClick={() => {
                  const go = confirm.run;
                  setConfirm(null);
                  go();
                }}
              >
                {confirm.label}
              </button>
            </div>
          </div>
        </div>
      )}
    </AppContext.Provider>
  );
}