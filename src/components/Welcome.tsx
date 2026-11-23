import { CheckCircle2, CircleAlert, Loader2 } from "lucide-react";
import type { ReactNode } from "react";
import { useApp } from "../context";
import type { Page } from "../types";

type Row = { ok: boolean | null; title: string; detail: string; fix?: { label: string; page: Page } };

function Tick({ ok }: { ok: boolean | null }): ReactNode {
  if (ok === null) return <Loader2 className="h-5 w-5 shrink-0 animate-spin text-slate-400" />;
  return ok ? (
    <CheckCircle2 className="h-5 w-5 shrink-0 text-emerald-500" />
  ) : (
    <CircleAlert className="h-5 w-5 shrink-0 text-amber-500" />
  );
}

/** First-run checklist: shows what is ready and what still needs one action. */
export default function Welcome({ onClose }: { onClose: () => void }) {
  const { qemu, images, go } = useApp();
  const verified = images.filter((i) => i.verified).length;

  const rows: Row[] = [
    {
      ok: qemu === null ? null : !!qemu.qemu,
      title: "QEMU is installed",
      detail: qemu === null ? "Checking..." : qemu.qemu ? "Found. Sandboxes can run." : "Not found. Install QEMU or set its path in Settings.",
      fix: { label: "Open Settings", page: "settings" },
    },
    {
      ok: qemu === null ? null : !!qemu.whpx_listed,
      title: "Hardware acceleration (WHPX)",
      detail:
        qemu === null
          ? "Checking..."
          : qemu.whpx_listed
            ? "Available. Sandboxes will run at full speed."
            : "Not available. Turn on Windows Hypervisor Platform in Windows Features, then restart.",
    },
    {
      ok: verified > 0,
      title: "A verified OS image",
      detail: verified > 0 ? `${verified} verified image${verified > 1 ? "s" : ""} ready.` : "Add an OS image and verify it once.",
      fix: { label: "Go to OS Images", page: "images" },
    },
  ];

  const ready = rows.every((r) => r.ok === true);
  const next = rows.find((r) => r.ok === false && r.fix);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4" role="dialog" aria-modal="true">
      <div className="app-card w-full max-w-lg p-6">
        <div className="flex items-center gap-3">
          <img src="/logo-mark.png" alt="" className="h-10 w-auto" draggable={false} />
          <div>
            <h2 className="text-lg font-semibold">Welcome to FLARQEN</h2>
            <p className="app-muted text-sm">Disposable OS Sandbox - quick setup check</p>
          </div>
        </div>

        <ul className="mt-5 flex flex-col gap-3">
          {rows.map((r) => (
            <li key={r.title} className="flex items-start gap-3">
              <Tick ok={r.ok} />
              <div className="min-w-0">
                <div className="font-medium">{r.title}</div>
                <div className="app-muted text-sm">{r.detail}</div>
              </div>
            </li>
          ))}
        </ul>

        <div className="mt-6 flex justify-end gap-2">
          <button className="app-btn app-btn-ghost" onClick={onClose}>
            {ready ? "Close" : "Skip for now"}
          </button>
          {ready ? (
            <button
              className="app-btn"
              onClick={() => {
                onClose();
                go("create");
              }}
            >
              Create a sandbox
            </button>
          ) : next?.fix ? (
            <button
              className="app-btn"
              onClick={() => {
                onClose();
                go(next.fix!.page);
              }}
            >
              {next.fix.label}
            </button>
          ) : null}
        </div>
        <p className="app-muted mt-3 text-xs">You can open this check again any time from "Setup check" in the sidebar.</p>
      </div>
    </div>
  );
}