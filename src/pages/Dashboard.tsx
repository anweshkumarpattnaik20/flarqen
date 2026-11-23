import { useEffect, useState } from "react";
import { api } from "../api";
import NoSandboxes from "../components/NoSandboxes";
import SandboxCard from "../components/SandboxCard";
import { Bar, PageTitle, Stat, fmtMb } from "../components/ui";
import { useApp } from "../context";
import type { HostMetrics, StorageInfo } from "../types";

export default function Dashboard() {
  const { sandboxes, images } = useApp();
  const [host, setHost] = useState<HostMetrics | null>(null);
  const [storage, setStorage] = useState<StorageInfo | null>(null);

  useEffect(() => {
    let alive = true;
    const load = async () => {
      if (document.hidden) return;
      try {
        const [h, s] = await Promise.all([api.host(), api.storage()]);
        if (alive) {
          setHost(h);
          setStorage(s);
        }
      } catch {
        /* backend problems are shown in the banner */
      }
    };
    void load();
    const t = setInterval(load, 5000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  const running = sandboxes.filter((s) => s.status === "running").length;
  const stopped = sandboxes.filter((s) => s.status === "stopped").length;
  const verified = images.filter((i) => i.verified).length;
  const ramPct = host && host.total_ram_mb > 0 ? (host.used_ram_mb / host.total_ram_mb) * 100 : 0;

  return (
    <div>
      <PageTitle title="Dashboard" sub="Live status from the backend" />
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Stat label="Total sandboxes" value={sandboxes.length} />
        <Stat label="Running" value={running} />
        <Stat label="Stopped" value={stopped} />
        <Stat label="Available OS images" value={verified} sub="verified" />
        <Stat
          label="Host CPU"
          value={host ? `${host.cpu_percent.toFixed(0)}%` : "-"}
          sub={host ? `${host.cpu_threads} threads` : undefined}
        />
        <Stat
          label="Host RAM"
          value={host ? `${(host.used_ram_mb / 1024).toFixed(1)} / ${(host.total_ram_mb / 1024).toFixed(1)} GB` : "-"}
          sub={host ? <Bar pct={ramPct} /> : undefined}
        />
        <Stat
          label="Disk used by sandboxes"
          value={storage ? fmtMb(storage.sandboxes_used_mb) : "-"}
          sub={storage ? `master images: ${fmtMb(storage.images_used_mb)} · ${storage.disk_free_gb} GB free` : undefined}
        />
      </div>

      <h2 className="mt-6 mb-2 text-lg font-semibold">Sandboxes</h2>
      {sandboxes.length === 0 ? (
        <NoSandboxes />
      ) : (
        <div className="grid gap-3 lg:grid-cols-2">
          {sandboxes.map((s) => (
            <SandboxCard key={s.id} s={s} />
          ))}
        </div>
      )}
    </div>
  );
}