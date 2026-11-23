import { PageTitle } from "../components/ui";

export default function Help() {
  return (
    <div className="max-w-2xl space-y-4 text-sm">
      <PageTitle title="Help and Documentation" />

      <div className="app-card p-4">
        <h2 className="mb-1 font-semibold">How it works</h2>
        <p className="app-muted">
          You install an operating system once into a <b>master image</b>, which is then made read-only. Every sandbox
          is a separate writable disk layered on top of that master. Resetting a sandbox throws away its disk and
          creates a fresh one, so the guest returns to the clean baseline without reinstalling.
        </p>
      </div>

      <div className="app-card p-4">
        <h2 className="mb-1 font-semibold">First time</h2>
        <ol className="app-muted list-decimal pl-5">
          <li>Go to OS Images and prepare a master image from an ISO you downloaded yourself.</li>
          <li>Go to Create Sandbox and walk through the wizard.</li>
          <li>Start the sandbox from My Sandboxes. The VM opens in its own QEMU window.</li>
          <li>Stop it gracefully when done. Use Reset to return it to the clean state, or Delete to remove it.</li>
        </ol>
      </div>

      <div className="app-card p-4">
        <h2 className="mb-1 font-semibold">Safety limits</h2>
        <ul className="app-muted list-disc pl-5">
          <li>Sandboxes have no network, no shared folders, no clipboard sharing and no USB passthrough.</li>
          <li>Force stop cuts the power; unsaved work in the guest is lost and its filesystem may be left inconsistent.</li>
          <li>Reset and Delete are permanent for that sandbox. The master image and other sandboxes are never touched.</li>
          <li>
            Virtualization reduces risk but does not guarantee isolation. A compromised guest or a QEMU vulnerability
            could affect your PC. This is not a replacement for a dedicated malware-analysis lab.
          </li>
        </ul>
      </div>

      <div className="app-card p-4">
        <h2 className="mb-1 font-semibold">Troubleshooting</h2>
        <ul className="app-muted list-disc pl-5">
          <li>VM will not start: open the sandbox's Details and load the VM log.</li>
          <li>Very slow: set acceleration to WHPX in Settings and make sure Windows Hypervisor Platform is enabled.</li>
          <li>Something failed: Activity Logs shows what happened; "Copy diagnostics" gathers it for sharing.</li>
        </ul>
      </div>

      <p className="app-muted text-xs">This is a development build, not a production release.</p>
    </div>
  );
}