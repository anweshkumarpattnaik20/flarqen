import { useEffect, useState } from "react";
import { api } from "../api";
import { PageTitle, fmtMb } from "../components/ui";
import { useApp } from "../context";
import type { HostMetrics, StorageInfo } from "../types";

const STEPS = ["Details", "OS image", "Resources", "Storage", "Review"];

type Template = {
  id: string;
  title: string;
  text: string;
  cpu: number | null; // null = the default from Settings
  ramMb: number | null;
  description: string;
  autoDeleteMinutes: number | null;
};

const TEMPLATES: Template[] = [
  { id: "blank", title: "Blank", text: "Your default settings, nothing preset.", cpu: null, ramMb: null, description: "", autoDeleteMinutes: null },
  {
    id: "quick",
    title: "Quick throwaway",
    text: "Small and short-lived. Deleted after 1 hour.",
    cpu: 1,
    ramMb: 1024,
    description: "Short-lived throwaway sandbox",
    autoDeleteMinutes: 60,
  },
  {
    id: "risky",
    title: "Try something risky",
    text: "Test an unknown file or a risky setting. Deleted after 24 hours.",
    cpu: 2,
    ramMb: 2048,
    description: "Test something risky; reset or delete afterwards",
    autoDeleteMinutes: 1440,
  },
  {
    id: "software",
    title: "Software testing",
    text: "More memory for installing and trying programs.",
    cpu: 2,
    ramMb: 4096,
    description: "Install and test software",
    autoDeleteMinutes: null,
  },
  {
    id: "heavy",
    title: "Heavy workload",
    text: "As much CPU and memory as this PC safely allows.",
    cpu: 4,
    ramMb: 6144,
    description: "Heavier workload",
    autoDeleteMinutes: null,
  },
];

const AUTO_DELETE_CHOICES: { label: string; minutes: number | null }[] = [
  { label: "Never", minutes: null },
  { label: "After 1 hour", minutes: 60 },
  { label: "After 8 hours", minutes: 480 },
  { label: "After 24 hours", minutes: 1440 },
  { label: "After 7 days", minutes: 10080 },
];

function autoDeleteLabel(minutes: number | null): string {
  return AUTO_DELETE_CHOICES.find((c) => c.minutes === minutes)?.label ?? "Never";
}

function nameProblem(name: string, existing: string[]): string | null {
  if (name.length === 0) return "Enter a name";
  if (name.length > 40) return "Use 40 characters or fewer";
  if (name !== name.trim()) return "No leading or trailing spaces";
  if (!/^[A-Za-z0-9 _-]+$/.test(name)) return "Use only letters, numbers, spaces, - and _";
  if (existing.some((n) => n.toLowerCase() === name.toLowerCase())) return "A sandbox with this name already exists";
  return null;
}

export default function CreateWizard() {
  const { images, sandboxes, settings, run, go, busyIds } = useApp();
  const busy = busyIds.includes("global");
  const [step, setStep] = useState(0);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [imageId, setImageId] = useState("");
  const [cpu, setCpu] = useState(settings.default_cpu);
  const [ramMb, setRamMb] = useState(settings.default_ram_mb);
  const [host, setHost] = useState<HostMetrics | null>(null);
  const [storage, setStorage] = useState<StorageInfo | null>(null);
  const [templateId, setTemplateId] = useState("blank");
  const [autoMins, setAutoMins] = useState<number | null>(null);

  useEffect(() => {
    api.host().then(setHost).catch(() => undefined);
    api.storage().then(setStorage).catch(() => undefined);
  }, []);

  const verified = images.filter((i) => i.verified);
  const chosenId = imageId || verified[0]?.id || "";
  const chosen = verified.find((i) => i.id === chosenId);
  const maxCpu = host ? Math.max(1, Math.floor(host.cpu_threads / 2)) : 2;
  const maxRam = host ? Math.floor(host.total_ram_mb / 2) : 4096;

  function applyTemplate(t: Template) {
    setTemplateId(t.id);
    setCpu(Math.min(t.cpu ?? settings.default_cpu, maxCpu));
    setRamMb(Math.min(t.ramMb ?? settings.default_ram_mb, maxRam));
    setDescription(t.description);
    setAutoMins(t.autoDeleteMinutes);
  }

  const problem = nameProblem(
    name,
    sandboxes.map((s) => s.name),
  );
  const resourcesOk = cpu >= 1 && cpu <= maxCpu && ramMb >= 512 && ramMb <= maxRam;
  const canNext = step === 0 ? problem === null : step === 1 ? !!chosen : step === 2 ? resourcesOk : true;

  async function create() {
    const sb = await run("Create sandbox", () =>
      api.create({ name, description, imageId: chosenId, cpu, ramMb }),
    );
    if (sb) {
      if (autoMins !== null) {
        await run(`Auto-delete timer for ${sb.name}`, () => api.setExpiry(sb.id, autoMins));
      }
      setName("");
      setDescription("");
      setTemplateId("blank");
      setAutoMins(null);
      setStep(0);
      go("sandboxes");
    }
  }

  return (
    <div className="max-w-2xl">
      <PageTitle title="Create Sandbox" sub="A new sandbox gets its own writable disk on top of a protected master image." />

      <div className="mb-4 flex gap-2 text-xs">
        {STEPS.map((s, i) => (
          <div
            key={s}
            className={`rounded px-2 py-1 ${i === step ? "app-btn" : "app-card"} ${i < step ? "opacity-70" : ""}`}
          >
            {i + 1}. {s}
          </div>
        ))}
      </div>

      <div className="app-card p-5">
        {step === 0 && (
          <div>
            <label className="app-label">Start from a template (you can change everything afterwards)</label>
            <div className="mb-4 grid gap-2 sm:grid-cols-2">
              {TEMPLATES.map((t) => (
                <button
                  key={t.id}
                  type="button"
                  className="app-card p-3 text-left"
                  style={templateId === t.id ? { borderColor: "var(--accent)", borderWidth: 2 } : undefined}
                  onClick={() => applyTemplate(t)}
                >
                  <div className="font-medium">{t.title}</div>
                  <div className="app-muted text-xs">{t.text}</div>
                </button>
              ))}
            </div>
            <label className="app-label">Sandbox name</label>
            <input className="app-input" value={name} onChange={(e) => setName(e.target.value)} placeholder="Lab 1" />
            {name.length > 0 && problem && <p className="mt-1 text-xs text-red-500">{problem}</p>}
            <label className="app-label">Description (optional)</label>
            <input
              className="app-input"
              value={description}
              maxLength={200}
              onChange={(e) => setDescription(e.target.value)}
            />
          </div>
        )}

        {step === 1 && (
          <div>
            {verified.length === 0 ? (
              <p className="app-muted">
                No verified image is available. Prepare one on the <b>OS Images</b> page first. Unverified or
                incomplete images cannot be used.
              </p>
            ) : (
              verified.map((im) => (
                <label key={im.id} className="app-card mb-2 flex cursor-pointer items-center gap-3 p-3">
                  <input type="radio" checked={chosenId === im.id} onChange={() => setImageId(im.id)} />
                  <div>
                    <div className="font-medium">
                      {im.name} {im.version}
                    </div>
                    <div className="app-muted text-xs">
                      {im.image_format} · verified · checksum {im.checksum?.slice(0, 16)}...
                    </div>
                  </div>
                </label>
              ))
            )}
          </div>
        )}

        {step === 2 && (
          <div>
            <label className="app-label">Virtual CPU cores (1 to {maxCpu} on this PC)</label>
            <input
              className="app-input"
              type="number"
              value={cpu}
              onChange={(e) => setCpu(Number(e.target.value))}
            />
            <label className="app-label">RAM in MB (512 to {maxRam} on this PC)</label>
            <input
              className="app-input"
              type="number"
              step={256}
              value={ramMb}
              onChange={(e) => setRamMb(Number(e.target.value))}
            />
            {!resourcesOk && <p className="mt-2 text-xs text-red-500">These values are outside the allowed range.</p>}
            <label className="app-label">Delete this sandbox automatically</label>
            <select
              className="app-input"
              value={autoMins === null ? "never" : String(autoMins)}
              onChange={(e) => setAutoMins(e.target.value === "never" ? null : Number(e.target.value))}
            >
              {AUTO_DELETE_CHOICES.map((c) => (
                <option key={c.label} value={c.minutes === null ? "never" : String(c.minutes)}>
                  {c.label}
                </option>
              ))}
            </select>
            {autoMins !== null && (
              <p className="mt-1 text-xs text-amber-600 dark:text-amber-400">
                The sandbox and everything in it will be permanently deleted at that time, while the app is open.
              </p>
            )}
            <p className="app-muted mt-3 text-xs">
              The limits keep at least half of your CPU threads and RAM free for Windows. The backend checks them again
              before anything is created. Disk size is set by the master image. Starting values come from Settings.
            </p>
          </div>
        )}

        {step === 3 && (
          <div className="space-y-2 text-sm">
            <div>
              <span className="app-muted">Master images folder:</span>
              <div className="break-all">{storage?.images_dir ?? "..."}</div>
            </div>
            <div>
              <span className="app-muted">Sandbox disks folder:</span>
              <div className="break-all">{storage?.sandboxes_dir ?? "..."}</div>
            </div>
            <div>
              <span className="app-muted">Free disk space:</span> {storage ? `${storage.disk_free_gb} GB` : "..."}
            </div>
            <p className="app-muted text-xs">
              The disk starts tiny and grows only as the guest writes data. The master image is read-only and is never
              written to by a sandbox. Choosing a different storage location is not supported yet.
            </p>
          </div>
        )}

        {step === 4 && (
          <div className="space-y-1 text-sm">
            <div>
              <span className="app-muted">Name:</span> {name}
            </div>
            {description && (
              <div>
                <span className="app-muted">Description:</span> {description}
              </div>
            )}
            <div>
              <span className="app-muted">OS image:</span> {chosen ? `${chosen.name} ${chosen.version}` : "-"}
            </div>
            <div>
              <span className="app-muted">CPU:</span> {cpu} cores
            </div>
            <div>
              <span className="app-muted">RAM:</span> {fmtMb(ramMb)}
            </div>
            <div>
              <span className="app-muted">Disk:</span> same capacity as the master image
            </div>
            <div>
              <span className="app-muted">Auto-delete:</span> {autoDeleteLabel(autoMins)}
            </div>
            <div>
              <span className="app-muted">Network:</span> none (the guest has no network access)
            </div>
            {busy && <p className="app-muted pt-2 text-xs">Creating the sandbox disk and checking it...</p>}
          </div>
        )}
      </div>

      <div className="mt-4 flex justify-between">
        <button className="app-btn app-btn-ghost" disabled={step === 0 || busy} onClick={() => setStep(step - 1)}>
          Back
        </button>
        {step < STEPS.length - 1 ? (
          <button className="app-btn" disabled={!canNext} onClick={() => setStep(step + 1)}>
            Next
          </button>
        ) : (
          <button className="app-btn" disabled={busy || !chosen || problem !== null || !resourcesOk} onClick={create}>
            Create sandbox
          </button>
        )}
      </div>
    </div>
  );
}