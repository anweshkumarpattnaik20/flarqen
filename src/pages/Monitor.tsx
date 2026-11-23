import { useEffect, useState } from "react";
import { api } from "../api";
import { Bar, PageTitle, Stat, fmtDur, fmtMb } from "../components/ui";
import type { HostMetrics, SandboxMetrics } from "../types";

export default function Monitor() {
  const [host, setHost] = useState<HostMetrics | null>(null);
  const [rows, setRows] = useState<SandboxMetrics[]>([]);

  useEffect(() => {
    let alive = true;
    const load = async () => {
      if (document.hidden) return;
      try {
        const [h, r] = await Promise.all([api.host(), api.sandboxMetrics()]);
        if (alive) {
          setHost(h);
          setRows(r);
        }
      } catch {
        /* shown in the banner */
      }
    };
    void load();
    const t = setInterval(load, 5000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  const ramPct = host && host.total_ram_mb > 0 ? (host.used_ram_mb / host.total_ram_mb) * 100 : 0;
  const diskUsed = host ? host.disk_total_gb - host.disk_free_gb : 0;
  const diskPct = host && host.disk_total_gb > 0 ? (diskUsed / host.disk_total_gb) * 100 : 0;

  return (
    <div>
      <PageTitle title="Resource Monitor" sub="Refreshes every 5 seconds while this page is open" />
      <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
        <Stat
          label="Host CPU"
          value={host ? `${host.cpu_percent.toFixed(0)}%` : "-"}
          sub={host ? <><span>{host.cpu_threads} threads</span><Bar pct={host.cpu_percent} /></> : undefined}
        />
        <Stat
          label="Host RAM"
          value={host ? `${(host.used_ram_mb / 1024).toFixed(1)} / ${(host.total_ram_mb / 1024).toFixed(1)} GB` : "-"}
          sub={host ? <><span>{(host.available_ram_mb / 1024).toFixed(1)} GB available</span><Bar pct={ramPct} /></> : undefined}
        />
        <Stat
          label="Host disk"
          value={host ? `${host.disk_free_gb} GB free` : "-"}
          sub={host ? <><span>of {host.disk_total_gb} GB</span><Bar pct={diskPct} /></> : undefined}
        />
      </div>

      <h2 className="mt-6 mb-2 text-lg font-semibold">Sandboxes</h2>
      <div className="app-card overflow-x-auto">
        <table className="w-full text-left text-sm">
          <thead className="app-muted text-xs">
            <tr>
              <th className="p-2">Name</th>
              <th className="p-2">Status</th>
              <th className="p-2">Configured</th>
              <th className="p-2">Measured RAM (host)</th>
              <th className="p-2">Disk file</th>
              <th className="p-2">Uptime</th>
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 && (
              <tr>
                <td className="app-muted p-2" colSpan={6}>
                  No sandboxes.
                </td>
              </tr>
            )}
            {rows.map((r) => (
              <tr key={r.id} className="border-t" style={{ borderColor: "var(--border)" }}>
                <td className="p-2">{r.name}</td>
                <td className="p-2">{r.status}</td>
                <td className="p-2">
                  {r.configured_cpu} CPU · {fmtMb(r.configured_ram_mb)}
                </td>
                <td className="p-2">{r.memory_mb !== null ? fmtMb(r.memory_mb) : "-"}</td>
                <td className="p-2">{fmtMb(r.overlay_mb)}</td>
                <td className="p-2">{fmtDur(r.uptime_secs)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="app-muted mt-3 text-xs">
        "Configured" is what the guest was given. "Measured RAM" is the memory the QEMU process uses on this PC, and
        "Disk file" is the real size of the sandbox's disk file. Guest-internal usage and per-VM CPU are not shown yet.
      </p>
    </div>
  );
}