import { useEffect, useState } from "react";
import { api } from "../api";
import { PageTitle, fmtMb } from "../components/ui";
import { useApp } from "../context";
import type { Theme } from "../context";
import type { StorageInfo } from "../types";

export default function Settings() {
  const { qemu, refreshQemu, theme, setTheme, settings, saveSettings, run, busyIds } = useApp();
  const busy = busyIds.includes("global");
  const [storage, setStorage] = useState<StorageInfo | null>(null);
  const [selfTest, setSelfTest] = useState<unknown>(null);
  const [form, setForm] = useState({
    qemuPath: "",
    qemuImgPath: "",
    cpu: 2,
    ram: 2048,
    accel: "whpx",
    days: 90,
  });

  useEffect(() => {
    api.storage().then(setStorage).catch(() => undefined);
  }, []);

  useEffect(() => {
    setForm({
      qemuPath: settings.qemu_path ?? "",
      qemuImgPath: settings.qemu_img_path ?? "",
      cpu: settings.default_cpu,
      ram: settings.default_ram_mb,
      accel: settings.acceleration,
      days: settings.log_retention_days,
    });
  }, [settings]);

  async function save() {
    await saveSettings({
      qemu_path: form.qemuPath.trim() ? form.qemuPath.trim() : null,
      qemu_img_path: form.qemuImgPath.trim() ? form.qemuImgPath.trim() : null,
      default_cpu: form.cpu,
      default_ram_mb: form.ram,
      acceleration: form.accel,
      log_retention_days: form.days,
    });
  }

  return (
    <div className="max-w-2xl">
      <PageTitle title="Settings" />

      <h2 className="mb-2 text-lg font-semibold">QEMU</h2>
      <div className="app-card space-y-1 p-4 text-sm">
        <div>
          <span className="app-muted">qemu-system:</span> {qemu?.qemu ? qemu.qemu.version : "not found"}
        </div>
        <div className="app-muted break-all text-xs">{qemu?.qemu?.path}</div>
        <div>
          <span className="app-muted">qemu-img:</span> {qemu?.qemu_img ? qemu.qemu_img.version : "not found"}
        </div>
        <div className="app-muted break-all text-xs">{qemu?.qemu_img?.path}</div>
        <div>
          <span className="app-muted">WHPX acceleration listed by QEMU:</span> {qemu?.whpx_listed ? "yes" : "no"}
        </div>

        <label className="app-label">Custom qemu-system-x86_64.exe path (leave empty to auto-detect)</label>
        <input
          className="app-input"
          placeholder="C:\Program Files\qemu\qemu-system-x86_64.exe"
          value={form.qemuPath}
          onChange={(e) => setForm({ ...form, qemuPath: e.target.value })}
        />
        <label className="app-label">Custom qemu-img.exe path (leave empty to auto-detect)</label>
        <input
          className="app-input"
          placeholder="C:\Program Files\qemu\qemu-img.exe"
          value={form.qemuImgPath}
          onChange={(e) => setForm({ ...form, qemuImgPath: e.target.value })}
        />
        <p className="app-muted pt-1 text-xs">
          A custom path must point to a file with the right name that identifies itself as QEMU. If a custom path stops
          working later, the tool is shown as "not found" until you fix or clear the path.
        </p>

        <div className="flex gap-2 pt-2">
          <button className="app-btn app-btn-ghost" onClick={() => void refreshQemu()}>
            Detect again
          </button>
          <button
            className="app-btn app-btn-ghost"
            disabled={busy}
            onClick={async () => {
              const r = await run("Disk self-test", () => api.diskSelfTest());
              if (r !== undefined) setSelfTest(r);
            }}
          >
            Run disk self-test
          </button>
        </div>
        {selfTest !== null && (
          <pre className="app-input mt-2 overflow-auto text-xs">{JSON.stringify(selfTest, null, 2)}</pre>
        )}
      </div>

      <h2 className="mt-6 mb-2 text-lg font-semibold">Virtual machine defaults</h2>
      <div className="app-card p-4">
        <div className="grid grid-cols-3 gap-x-3">
          <div>
            <label className="app-label">Default CPU cores</label>
            <input
              className="app-input"
              type="number"
              value={form.cpu}
              onChange={(e) => setForm({ ...form, cpu: Number(e.target.value) })}
            />
          </div>
          <div>
            <label className="app-label">Default RAM (MB)</label>
            <input
              className="app-input"
              type="number"
              step={256}
              value={form.ram}
              onChange={(e) => setForm({ ...form, ram: Number(e.target.value) })}
            />
          </div>
          <div>
            <label className="app-label">Acceleration</label>
            <select
              className="app-input"
              value={form.accel}
              onChange={(e) => setForm({ ...form, accel: e.target.value })}
            >
              <option value="whpx">WHPX (fast)</option>
              <option value="tcg">TCG (slow, fallback)</option>
            </select>
          </div>
        </div>
        <label className="app-label">Keep activity log entries for (days)</label>
        <input
          className="app-input w-40"
          type="number"
          value={form.days}
          onChange={(e) => setForm({ ...form, days: Number(e.target.value) })}
        />
        <div className="mt-4">
          <button className="app-btn" disabled={busy} onClick={() => void save()}>
            Save settings
          </button>
        </div>
        <p className="app-muted mt-2 text-xs">
          The backend checks every value before saving. Defaults apply to new sandboxes in the wizard; acceleration
          applies the next time a VM is started.
        </p>
      </div>

      <h2 className="mt-6 mb-2 text-lg font-semibold">Appearance</h2>
      <div className="app-card p-4">
        <label className="app-label">Theme (saved on this PC)</label>
        <select className="app-input w-48" value={theme} onChange={(e) => setTheme(e.target.value as Theme)}>
          <option value="system">Follow system</option>
          <option value="light">Light</option>
          <option value="dark">Dark</option>
        </select>
      </div>

      <h2 className="mt-6 mb-2 text-lg font-semibold">Storage</h2>
      <div className="app-card space-y-1 p-4 text-sm">
        <div>
          <span className="app-muted">Application data:</span> <span className="break-all">{storage?.data_dir}</span>
        </div>
        <div>
          <span className="app-muted">Master images:</span> {storage ? fmtMb(storage.images_used_mb) : "..."}
        </div>
        <div>
          <span className="app-muted">Sandbox disks:</span> {storage ? fmtMb(storage.sandboxes_used_mb) : "..."}
        </div>
        <div>
          <span className="app-muted">Free disk space:</span> {storage ? `${storage.disk_free_gb} GB` : "..."}
        </div>
      </div>
    </div>
  );
}