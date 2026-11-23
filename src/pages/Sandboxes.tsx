import NoSandboxes from "../components/NoSandboxes";
import SandboxCard from "../components/SandboxCard";
import { PageTitle } from "../components/ui";
import { useApp } from "../context";

export default function Sandboxes() {
  const { sandboxes, go } = useApp();
  return (
    <div>
      <div className="flex items-start justify-between">
        <PageTitle title="My Sandboxes" sub="Each sandbox has its own disk; the master image is never modified." />
        <button className="app-btn" onClick={() => go("create")}>
          New sandbox
        </button>
      </div>
      {sandboxes.length === 0 ? (
        <NoSandboxes />
      ) : (
        <div className="grid gap-3">
          {sandboxes.map((s) => (
            <SandboxCard key={s.id} s={s} />
          ))}
        </div>
      )}
      <p className="app-muted mt-4 text-xs">
        A VM opens in its own QEMU window when it starts. Stop a sandbox before duplicating it.
      </p>
    </div>
  );
}