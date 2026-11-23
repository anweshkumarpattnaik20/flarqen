import { useApp } from "../context";

/** Friendly empty screen: explains the single next step and gives one button for it. */
export default function NoSandboxes() {
  const { qemu, images, go } = useApp();
  const verified = images.filter((i) => i.verified).length;

  let title = "Create your first sandbox";
  let text = "A sandbox is a throw-away computer. Break anything inside it, then reset it in seconds.";
  let button = "Create sandbox";
  let target: "create" | "images" | "settings" = "create";

  if (qemu && !qemu.qemu) {
    title = "QEMU was not found";
    text = "Sandboxes run on QEMU. Install it, or tell the app where qemu-system-x86_64.exe is.";
    button = "Open Settings";
    target = "settings";
  } else if (verified === 0) {
    title = "Prepare an OS image first";
    text = "Every sandbox starts from a read-only master image. Add one and verify it once.";
    button = "Go to OS Images";
    target = "images";
  }

  return (
    <div className="app-card flex flex-col items-center px-6 py-10 text-center">
      <img src="/logo-mark.png" alt="" className="h-16 w-auto opacity-90" draggable={false} />
      <h3 className="mt-4 text-lg font-semibold">{title}</h3>
      <p className="app-muted mt-1 max-w-md text-sm">{text}</p>
      <button className="app-btn mt-4" onClick={() => go(target)}>
        {button}
      </button>
    </div>
  );
}