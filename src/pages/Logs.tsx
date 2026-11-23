import { useCallback, useEffect, useState } from "react";
import { api, errText } from "../api";
import { PageTitle } from "../components/ui";
import { useApp } from "../context";
import type { ActivityEntry } from "../types";

const color: Record<string, string> = {
  ok: "text-emerald-500",
  warning: "text-amber-500",
  error: "text-red-500",
};

export default function Logs() {
  const { qemu, notify } = useApp();
  const [rows, setRows] = useState<ActivityEntry[]>([]);
  const [filter, setFilter] = useState("all");

  const load = useCallback(async () => {
    try {
      setRows(await api.activity(200));
    } catch (e) {
      notify("error", `Could not load the log: ${errText(e)}`);
    }
  }, [notify]);

  useEffect(() => {
    void load();
    const t = setInterval(() => {
      if (!document.hidden) void load();
    }, 5000);
    return () => clearInterval(t);
  }, [load]);

  const shown = rows.filter((r) => filter === "all" || r.status === filter);

  async function copy() {
    const header =
      "Disposable OS Sandbox diagnostics (development build)\n" +
      `QEMU: ${qemu?.qemu?.version ?? "not found"}\n` +
      `qemu-img: ${qemu?.qemu_img?.version ?? "not found"}\n` +
      `WHPX listed by QEMU: ${qemu?.whpx_listed ? "yes" : "no"}\n\n`;
    const body = shown
      .map(
        (r) =>
          `${new Date(r.timestamp * 1000).toISOString()} [${r.status}] ${r.operation}` +
          `${r.sandbox_id ? ` (${r.sandbox_id})` : ""}: ${r.message}`,
      )
      .join("\n");
    try {
      await navigator.clipboard.writeText(header + body);
      notify("success", "Diagnostics copied. Review them before sharing.");
    } catch {
      notify("error", "Could not copy to the clipboard");
    }
  }

  return (
    <div>
      <PageTitle title="Activity Logs" sub="Every create, start, stop, reset, delete and failure is recorded here" />
      <div className="mb-3 flex items-center gap-2">
        <select className="app-input w-40" value={filter} onChange={(e) => setFilter(e.target.value)}>
          <option value="all">All</option>
          <option value="ok">ok</option>
          <option value="warning">warning</option>
          <option value="error">error</option>
        </select>
        <button className="app-btn app-btn-ghost" onClick={() => void load()}>
          Refresh
        </button>
        <button className="app-btn app-btn-ghost" onClick={copy}>
          Copy diagnostics
        </button>
      </div>
      <div className="app-card overflow-x-auto">
        <table className="w-full text-left text-xs">
          <thead className="app-muted">
            <tr>
              <th className="p-2">Time</th>
              <th className="p-2">Status</th>
              <th className="p-2">Operation</th>
              <th className="p-2">Message</th>
            </tr>
          </thead>
          <tbody>
            {shown.length === 0 && (
              <tr>
                <td className="app-muted p-2" colSpan={4}>
                  Nothing to show.
                </td>
              </tr>
            )}
            {shown.map((r) => (
              <tr key={r.id} className="border-t" style={{ borderColor: "var(--border)" }}>
                <td className="p-2 whitespace-nowrap">{new Date(r.timestamp * 1000).toLocaleString()}</td>
                <td className={`p-2 ${color[r.status] ?? ""}`}>{r.status}</td>
                <td className="p-2">{r.operation}</td>
                <td className="p-2">{r.message}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}