import { useEffect, useState } from "react";
import { api } from "../api";
import { PageTitle, fmtTime } from "../components/ui";
import { useApp } from "../context";
import type { InstallRequest, InstallStarted, InstallStatus, VerifyReport } from "../types";

export default function Images() {
  const { images, run, busyIds, ask, accel } = useApp();
  const busy = busyIds.includes("global");
  const [reports, setReports] = useState<Record<string, VerifyReport>>({});
  const [form, setForm] = useState<InstallRequest>({
    name: "Ubuntu",
    version: "24.04",
    isoPath: "",
    diskGb: 25,
    ramMb: 4096,
    cpu: 2,
    accel,
    expectedSha256: "",
  });
  const [status, setStatus] = useState<InstallStatus | null>(null);
  const [started, setStarted] = useState<InstallStarted | null>(null);

  useEffect(() => {
    let alive = true;
    const load = async () => {
      if (document.hidden) return;
      try {
        const s = await api.installStatus();
        if (alive) setStatus(s);
      } catch {
        /* shown in the banner */
      }
    };
    void load();
    const t = setInterval(load, 3000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  async function verify(id: string) {
    const r = await run("Verify master image", () => api.verify(id), id);
    if (r) setReports((p) => ({ ...p, [id]: r }));
  }

  async function startInstall() {
    const r = await run("Start installer VM", () => api.installStart(form));
    if (r) setStarted(r);
  }

  const set = <K extends keyof InstallRequest>(k: K, v: InstallRequest[K]) => setForm({ ...form, [k]: v });

  return (
    <div className="max-w-3xl">
      <PageTitle title="OS Images" sub="Master images are read-only. Sandboxes only ever write to their own disk." />

      {images.length === 0 && <p className="app-muted">No images yet. Prepare one below.</p>}
      {images.map((im) => {
        const r = reports[im.id];
        return (
          <div key={im.id} className="app-card mb-2 p-3">
            <div className="flex items-center justify-between">
              <div>
                <div className="font-medium">
                  {im.name} {im.version} <span className="app-muted text-xs">({im.verified ? "verified" : "unverified"})</span>
                </div>
                <div className="app-muted text-xs">
                  {im.image_format} · registered {fmtTime(im.created_at)}
                </div>
                <div className="app-muted break-all text-xs">sha256 {im.checksum}</div>
              </div>
              <button
                className="app-btn app-btn-ghost"
                disabled={busyIds.includes(im.id)}
                onClick={() => verify(im.id)}
              >
                {busyIds.includes(im.id) ? "Hashing..." : "Verify master"}
              </button>
            </div>
            {r && (
              <p className={`mt-2 text-xs ${r.matches ? "text-emerald-500" : "text-red-500"}`}>
                {r.matches ? "Checksum matches the value recorded at install time" : "CHECKSUM MISMATCH - do not use this image"}
                {" · "}
                {r.read_only ? "read-only on disk" : "NOT read-only on disk"}
              </p>
            )}
          </div>
        );
      })}

      <h2 className="mt-6 mb-2 text-lg font-semibold">Prepare a new master image</h2>
      <div className="app-card p-4">
        <ol className="app-muted mb-3 list-decimal pl-5 text-xs">
          <li>Download an installer ISO from the vendor and copy its published SHA-256 checksum.</li>
          <li>Start the installer VM here, then install the OS in the QEMU window. The VM has no network.</li>
          <li>When the installer says it is complete, close the QEMU window (do not boot the installed system).</li>
          <li>Click "Finish and register" to verify and protect the image.</li>
        </ol>
        <div className="grid grid-cols-2 gap-x-3">
          <div>
            <label className="app-label">OS name (letters, numbers, spaces)</label>
            <input className="app-input" value={form.name} onChange={(e) => set("name", e.target.value)} />
          </div>
          <div>
            <label className="app-label">Version</label>
            <input className="app-input" value={form.version} onChange={(e) => set("version", e.target.value)} />
          </div>
        </div>
        <label className="app-label">ISO file path (no commas)</label>
        <input
          className="app-input"
          placeholder="C:\isos\ubuntu.iso"
          value={form.isoPath}
          onChange={(e) => set("isoPath", e.target.value)}
        />
        <label className="app-label">Published SHA-256 of the ISO (strongly recommended)</label>
        <input
          className="app-input"
          value={form.expectedSha256}
          onChange={(e) => set("expectedSha256", e.target.value)}
        />
        <div className="grid grid-cols-4 gap-x-3">
          <div>
            <label className="app-label">Disk GB</label>
            <input className="app-input" type="number" value={form.diskGb} onChange={(e) => set("diskGb", Number(e.target.value))} />
          </div>
          <div>
            <label className="app-label">RAM MB</label>
            <input className="app-input" type="number" value={form.ramMb} onChange={(e) => set("ramMb", Number(e.target.value))} />
          </div>
          <div>
            <label className="app-label">CPU cores</label>
            <input className="app-input" type="number" value={form.cpu} onChange={(e) => set("cpu", Number(e.target.value))} />
          </div>
          <div>
            <label className="app-label">Acceleration</label>
            <select className="app-input" value={form.accel} onChange={(e) => set("accel", e.target.value)}>
              <option value="whpx">whpx (fast)</option>
              <option value="tcg">tcg (slow)</option>
            </select>
          </div>
        </div>

        <div className="mt-4 flex flex-wrap gap-2">
          <button className="app-btn" disabled={busy || !form.isoPath} onClick={startInstall}>
            1. Start installer VM
          </button>
          <button
            className="app-btn"
            disabled={busy}
            onClick={async () => {
              const img = await run("Finish and register image", () => api.installFinish());
              if (img) setStarted(null);
            }}
          >
            2. Finish and register
          </button>
          <button
            className="app-btn app-btn-danger"
            disabled={busy || !status?.active}
            onClick={() =>
              ask({
                title: "Cancel the installation?",
                body: "The installer VM is stopped and its partly installed disk is deleted. Existing images are not affected.",
                label: "Cancel install",
                run: () => void run("Cancel install", () => api.installCancel()),
              })
            }
          >
            Cancel install
          </button>
        </div>
        {busy && <p className="app-muted mt-2 text-xs">Working... hashing a large ISO can take a minute or two.</p>}

        {started && (
          <p className="app-muted mt-3 break-all text-xs">
            Installer started (pid {started.pid}). ISO SHA-256 {started.isoSha256} ·{" "}
            {started.isoChecksumVerified ? "matches the checksum you entered" : "NOT checked against a published checksum"}
          </p>
        )}
        {status?.active && (
          <div className="mt-3 text-xs">
            <div>
              Installer for "{status.name}":{" "}
              <b>{status.running ? "VM is running" : `VM closed (exit code ${status.exitCode ?? "?"}) - ready to finish`}</b>
            </div>
            {status.logTail && status.logTail.trim().length > 0 && (
              <pre className="app-input mt-2 max-h-32 overflow-auto whitespace-pre-wrap">{status.logTail}</pre>
            )}
          </div>
        )}
      </div>
    </div>
  );
}