import { useEffect, useRef, useState } from "react";

/** Short start-up screen with the logo. Click or press any key to skip. */
export default function Splash({ onDone }: { onDone: () => void }) {
  const [leaving, setLeaving] = useState(false);
  const done = useRef(onDone);
  done.current = onDone;

  useEffect(() => {
    const t1 = setTimeout(() => setLeaving(true), 1200);
    const t2 = setTimeout(() => done.current(), 1700);
    const skip = () => done.current();
    window.addEventListener("keydown", skip);
    return () => {
      clearTimeout(t1);
      clearTimeout(t2);
      window.removeEventListener("keydown", skip);
    };
  }, []);

  return (
    <div
      className={`splash-screen fixed inset-0 z-[60] flex cursor-pointer flex-col items-center justify-center ${
        leaving ? "splash-out" : ""
      }`}
      style={{ background: "var(--bg)" }}
      onClick={() => done.current()}
      role="presentation"
    >
      <img src="/logo-mark.png" alt="" className="splash-mark h-auto w-28" draggable={false} />
      <img src="/logo-wordmark-dark.png" alt="FLARQEN" className="mt-5 block h-auto w-56 dark:hidden" draggable={false} />
      <img src="/logo-wordmark-light.png" alt="FLARQEN" className="mt-5 hidden h-auto w-56 dark:block" draggable={false} />
      <div className="app-muted mt-2 text-[11px] font-medium uppercase tracking-[0.2em]">Disposable OS Sandbox</div>
      <div className="mt-8 h-0.5 w-40 overflow-hidden rounded" style={{ background: "var(--border)" }}>
        <div className="splash-bar h-full w-full origin-left" style={{ background: "var(--accent)" }} />
      </div>
    </div>
  );
}