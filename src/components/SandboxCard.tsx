import { useEffect, useState } from "react";
import { api } from "../api";
import { useApp } from "../context";
import type { Sandbox } from "../types";
import SnapshotPanel from "./SnapshotPanel";
import { StatusBadge, fmtDur, fmtTime } from "./ui";

const TIMER_CHOICES: { label: string; minutes: number }[] = [
  { label: "30 minutes", minutes: 30 },
  { label: "1 hour", minutes: 60 },
  { label: "2 hours", minutes: 120 },
  { label: "8 hours", minutes: 480 },
  { label: "24 hours", minutes: 1440 },
  { label: "7 days", minutes: 10080 },
];

export default function SandboxCard({ s }: { s: Sandbox }) {
  const { run, busyIds, ask, accel } = useApp();
  const [open, setOpen] = useState(false);
  const [log, setLog] = useState<string | null>(null);
  const [dupOpen, setDupOpen] = useState(false);
  const [dupName, setDupName] = useState("");
  const [timerOpen, setTimerOpen] = useState(false);
  const [timerMins, setTimerMins] = useState(120);
  const [expiresAt, setExpiresAt] = useState<number | null>(null);
  const [nowSec, setNowSec] = useState(() => Math.floor(Date.now() / 1000));
  const busy = busyIds.includes(s.id);
  const live = s.status === "running" || s.status === "stopping";

  useEffect(() => {
    let alive = true;
    api
      .expiry(s.id)
      .then((v) => {
        if (alive) setExpiresAt(v);
      })
      .catch(() => {
        /* problems with the timer are shown when it is changed */
      });
    return () => {
      alive = false;
    };
  }, [s.id, s.status]);

  useEffect(() => {
    const t = setInterval(() => setNowSec(Math.floor(Date.now() / 1000)), 15000);
    return () => clearInterval(t);
  }, []);

  async function loadLog() {
    const t = await run("Load VM log", () => api.vmLog(s.id), s.id);
    if (t !== undefined) setLog(t.length > 0 ? t : "(the VM log is empty)");
  }

  return (
    <div className="app-card p-4">
      <div className="flex items-center justify-between">
        <div className="font-medium">
          {s.name} <span className="ml-2"><StatusBadge status={s.status} /></span>
        </div>
        <div className="app-muted text-xs">{s.process_id ? `pid ${s.process_id}` : ""}</div>
      </div>
      <div className="app-muted mt-1 text-xs">
        {s.operating_system} {s.os_version} · {s.cpu_cores} CPU · {s.ram_mb} MB RAM · {s.disk_capacity_gb} GB disk
        <br />
        created {fmtTime(s.created_at)} · last started {fmtTime(s.last_started_at)}
      </div>

      <div className="mt-3 flex flex-wrap gap-2">
        <button
          className="app-btn"
          disabled={busy || s.status !== "stopped"}
          onClick={() => run(`Start ${s.name}`, () => api.start(s.id, accel), s.id)}
        >
          Start
        </button>
        <button
          className="app-btn app-btn-ghost"
          disabled={busy || !live}
          onClick={() => run(`Stop ${s.name}`, () => api.stop(s.id, false), s.id)}
        >
          Stop
        </button>
        <button
          className="app-btn app-btn-danger"
          disabled={busy || !live}
          onClick={() =>
            ask({
              title: `Force stop "${s.name}"?`,
              body: "This cuts the power to the VM immediately. Unsaved work inside the guest is lost and its filesystem may be left inconsistent. Reset will restore a clean state.",
              label: "Force stop",
              run: () => void run(`Force stop ${s.name}`, () => api.stop(s.id, true), s.id),
            })
          }
        >
          Force stop
        </button>
        <button
          className="app-btn app-btn-danger"
          disabled={busy || s.status !== "stopped"}
          onClick={() =>
            ask({
              title: `Reset "${s.name}"?`,
              body: "Everything changed inside this sandbox since creation will be discarded and it returns to the clean baseline. The master image and other sandboxes are not affected.",
              label: "Reset sandbox",
              run: () => void run(`Reset ${s.name}`, () => api.reset(s.id), s.id),
            })
          }
        >
          Reset
        </button>
        <button
          className="app-btn app-btn-danger"
          disabled={busy || !["stopped", "error"].includes(s.status)}
          onClick={() =>
            ask({
              title: `Delete "${s.name}"?`,
              body: "This permanently deletes this sandbox and all data inside it. It cannot be undone. The master image and other sandboxes are not affected.",
              label: "Delete sandbox",
              run: () => void run(`Delete ${s.name}`, () => api.remove(s.id), s.id),
            })
          }
        >
          Delete
        </button>
        <button
          className="app-btn app-btn-ghost"
          disabled={busy || s.status !== "stopped"}
          title="Make an independent copy of this sandbox, including its current state"
          onClick={() => {
            setDupName(`${s.name} copy`);
            setDupOpen(!dupOpen);
          }}
        >
          Duplicate
        </button>
        <button
          className="app-btn app-btn-ghost"
          disabled={busy}
          title="Delete this sandbox automatically after a set time"
          onClick={() => setTimerOpen(!timerOpen)}
        >
          Auto-delete
        </button>
        <button className="app-btn app-btn-ghost" onClick={() => setOpen(!open)}>
          {open ? "Hide details" : "Details"}
        </button>
      </div>

      <SnapshotPanel s={s} busy={busy} />

      {dupOpen && (
        <div className="mt-3 flex flex-wrap items-center gap-2 border-t pt-3" style={{ borderColor: "var(--border)" }}>
          <input
            className="app-input max-w-xs"
            aria-label="Name for the copy"
            value={dupName}
            maxLength={40}
            onChange={(e) => setDupName(e.target.value)}
          />
          <button
            className="app-btn"
            disabled={busy || dupName.trim().length === 0}
            onClick={async () => {
              const r = await run(`Duplicate ${s.name}`, () => api.duplicate(s.id, dupName.trim()), s.id);
              if (r) setDupOpen(false);
            }}
          >
            Create copy
          </button>
          <button className="app-btn app-btn-ghost" onClick={() => setDupOpen(false)}>
            Cancel
          </button>
          <span className="app-muted text-xs">The copy keeps everything changed so far. Large disks can take a while.</span>
        </div>
      )}

      {expiresAt !== null && (
        <p className="mt-2 text-xs text-amber-600 dark:text-amber-400">
          Auto-delete in {fmtDur(Math.max(0, expiresAt - nowSec))}. This sandbox and its data will be removed.
        </p>
      )}

      {timerOpen && (
        <div className="mt-3 border-t pt-3" style={{ borderColor: "var(--border)" }}>
          <div className="flex flex-wrap items-center gap-2">
            <select
              className="app-input w-40"
              aria-label="Delete after"
              value={timerMins}
              onChange={(e) => setTimerMins(Number(e.target.value))}
            >
              {TIMER_CHOICES.map((c) => (
                <option key={c.minutes} value={c.minutes}>
                  {c.label}
                </option>
              ))}
            </select>
            <button
              className="app-btn app-btn-danger"
              disabled={busy}
              onClick={() =>
                ask({
                  title: `Auto-delete "${s.name}"?`,
                  body: "When the time runs out this sandbox is shut down (if running) and permanently deleted with everything inside it. The master image and other sandboxes are not affected. The app must be open: if the time passes while it is closed, the sandbox is deleted the next time the app starts.",
                  label: "Start timer",
                  run: () =>
                    void (async () => {
                      const at = await run(`Auto-delete timer for ${s.name}`, () => api.setExpiry(s.id, timerMins), s.id);
                      if (at !== undefined) {
                        setExpiresAt(at);
                        setTimerOpen(false);
                      }
                    })(),
                })
              }
            >
              Start timer
            </button>
            {expiresAt !== null && (
              <button
                className="app-btn app-btn-ghost"
                disabled={busy}
                onClick={async () => {
                  const at = await run(`Remove timer from ${s.name}`, () => api.setExpiry(s.id, null), s.id);
                  if (at !== undefined) {
                    setExpiresAt(null);
                    setTimerOpen(false);
                  }
                }}
              >
                Cancel timer
              </button>
            )}
          </div>
          <p className="app-muted mt-2 text-xs">Works only while the app is open. A running sandbox is shut down first.</p>
        </div>
      )}

      {busy && <p className="app-muted mt-2 text-xs">Working... a graceful stop can take up to 90 seconds.</p>}

      {open && (
        <div className="mt-3 border-t pt-3 text-xs" style={{ borderColor: "var(--border)" }}>
          <div className="app-muted">ID: {s.id}</div>
          <div className="app-muted break-all">Disk file: {s.overlay_path}</div>
          {s.description && <div className="app-muted">Description: {s.description}</div>}
          <button className="app-btn app-btn-ghost mt-2" disabled={busy} onClick={loadLog}>
            Load VM log
          </button>
          {log !== null && (
            <pre className="app-input mt-2 max-h-48 overflow-auto whitespace-pre-wrap text-xs">{log}</pre>
          )}
        </div>
      )}
    </div>
  );
}