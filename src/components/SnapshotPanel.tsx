import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { SnapshotInfo } from "../api";
import { useApp } from "../context";
import type { Sandbox } from "../types";
import { fmtMb, fmtTime } from "./ui";

/** Saved checkpoints of one sandbox: take one, go back to one, or delete one. */
export default function SnapshotPanel({ s, busy }: { s: Sandbox; busy: boolean }) {
  const { run, ask } = useApp();
  const [open, setOpen] = useState(false);
  const [list, setList] = useState<SnapshotInfo[]>([]);
  const [name, setName] = useState("");
  const stopped = s.status === "stopped";

  const reload = useCallback(async () => {
    try {
      setList(await api.snapshots(s.id));
    } catch {
      /* problems are shown when an action is used */
    }
  }, [s.id]);

  useEffect(() => {
    void reload();
  }, [reload, s.status]);

  async function create() {
    const r = await run(`Snapshot of ${s.name}`, () => api.createSnapshot(s.id, name.trim()), s.id);
    if (r) setName("");
    await reload();
  }

  return (
    <div className="mt-2">
      <button className="app-btn app-btn-ghost" onClick={() => setOpen(!open)}>
        Snapshots ({list.length})
      </button>

      {open && (
        <div className="mt-2 border-t pt-3" style={{ borderColor: "var(--border)" }}>
          <div className="flex flex-wrap items-center gap-2">
            <input
              className="app-input max-w-xs"
              aria-label="Snapshot name"
              placeholder="Name, e.g. Clean install"
              value={name}
              maxLength={40}
              onChange={(e) => setName(e.target.value)}
            />
            <button className="app-btn" disabled={busy || !stopped || name.trim().length === 0} onClick={create}>
              Take snapshot
            </button>
            <span className="app-muted text-xs">
              {stopped ? "Saves the current state. Up to 5 per sandbox." : "Stop the sandbox to take or restore a snapshot."}
            </span>
          </div>

          {list.length === 0 ? (
            <p className="app-muted mt-3 text-sm">No snapshots yet.</p>
          ) : (
            <ul className="mt-3 flex flex-col gap-2">
              {list.map((sn) => (
                <li key={sn.id} className="flex flex-wrap items-center justify-between gap-2 text-sm">
                  <div>
                    <span className="font-medium">{sn.name}</span>
                    <span className="app-muted ml-2 text-xs">
                      {fmtTime(sn.created_at)} · {fmtMb(sn.size_bytes / 1048576)}
                    </span>
                  </div>
                  <div className="flex gap-2">
                    <button
                      className="app-btn app-btn-danger"
                      disabled={busy || !stopped}
                      onClick={() =>
                        ask({
                          title: `Restore "${sn.name}"?`,
                          body: `"${s.name}" goes back to the state saved in this snapshot. Everything changed since then is discarded. The snapshot itself is kept.`,
                          label: "Restore snapshot",
                          run: () =>
                            void (async () => {
                              await run(`Restore ${s.name} to ${sn.name}`, () => api.restoreSnapshot(s.id, sn.id), s.id);
                              await reload();
                            })(),
                        })
                      }
                    >
                      Restore
                    </button>
                    <button
                      className="app-btn app-btn-ghost"
                      disabled={busy}
                      onClick={() =>
                        ask({
                          title: `Delete snapshot "${sn.name}"?`,
                          body: "The saved copy is permanently deleted. The sandbox itself is not changed.",
                          label: "Delete snapshot",
                          run: () =>
                            void (async () => {
                              await run(`Delete snapshot ${sn.name}`, () => api.deleteSnapshot(s.id, sn.id), s.id);
                              await reload();
                            })(),
                        })
                      }
                    >
                      Delete
                    </button>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}