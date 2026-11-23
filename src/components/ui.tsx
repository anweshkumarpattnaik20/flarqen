import type { ReactNode } from "react";

const badgeColor: Record<string, string> = {
  running: "bg-emerald-600",
  stopped: "bg-slate-500",
  starting: "bg-amber-600",
  stopping: "bg-amber-600",
  creating: "bg-amber-600",
  resetting: "bg-amber-600",
  error: "bg-red-600",
  unknown: "bg-red-600",
};

export function StatusBadge({ status }: { status: string }) {
  return (
    <span
      className={`rounded px-2 py-0.5 text-xs font-medium text-white ${badgeColor[status] ?? "bg-slate-500"}`}
    >
      {status}
    </span>
  );
}

export function Stat({ label, value, sub }: { label: string; value: ReactNode; sub?: ReactNode }) {
  return (
    <div className="app-card p-4">
      <div className="app-muted text-xs">{label}</div>
      <div className="mt-1 text-2xl font-semibold">{value}</div>
      {sub ? <div className="app-muted mt-1 text-xs">{sub}</div> : null}
    </div>
  );
}

export function Bar({ pct }: { pct: number }) {
  const p = Math.max(0, Math.min(100, pct));
  const color = p > 90 ? "bg-red-500" : p > 75 ? "bg-amber-500" : "bg-sky-500";
  return (
    <div className="mt-2 h-2 w-full rounded bg-slate-400/30">
      <div className={`h-2 rounded ${color}`} style={{ width: `${p}%` }} />
    </div>
  );
}

export function PageTitle({ title, sub }: { title: string; sub?: string }) {
  return (
    <div className="mb-4">
      <h1 className="text-xl font-semibold">{title}</h1>
      {sub ? <p className="app-muted text-sm">{sub}</p> : null}
    </div>
  );
}

export function fmtTime(ts: number | null): string {
  return ts ? new Date(ts * 1000).toLocaleString() : "never";
}

export function fmtDur(secs: number | null): string {
  if (secs === null) return "-";
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  return h > 0 ? `${h}h ${m}m` : m > 0 ? `${m}m ${s}s` : `${s}s`;
}

export function fmtMb(mb: number): string {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;
}