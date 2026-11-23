import { useEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import { api } from "../api";
import { useApp } from "../context";
import type { Page } from "../types";

type Item = { id: string; label: string; hint: string; go: () => void };

const PAGES: { id: Page; label: string }[] = [
  { id: "dashboard", label: "Dashboard" },
  { id: "sandboxes", label: "My Sandboxes" },
  { id: "create", label: "Create Sandbox" },
  { id: "images", label: "OS Images" },
  { id: "monitor", label: "Resource Monitor" },
  { id: "logs", label: "Activity Logs" },
  { id: "settings", label: "Settings" },
  { id: "help", label: "Help" },
];

/** Ctrl+K palette: type a few letters, press Enter. */
export default function CommandBar({ onClose, onSetup }: { onClose: () => void; onSetup: () => void }) {
  const { sandboxes, go, run, accel, setTheme } = useApp();
  const [query, setQuery] = useState("");
  const [sel, setSel] = useState(0);
  const listRef = useRef<HTMLUListElement>(null);

  const items = useMemo<Item[]>(() => {
    const out: Item[] = [];
    for (const s of sandboxes) {
      if (s.status === "stopped") {
        out.push({
          id: `start-${s.id}`,
          label: `Start ${s.name}`,
          hint: "Sandbox",
          go: () => void run(`Start ${s.name}`, () => api.start(s.id, accel), s.id),
        });
      }
      if (s.status === "running") {
        out.push({
          id: `stop-${s.id}`,
          label: `Stop ${s.name}`,
          hint: "Sandbox",
          go: () => void run(`Stop ${s.name}`, () => api.stop(s.id, false), s.id),
        });
      }
    }
    for (const p of PAGES) out.push({ id: `go-${p.id}`, label: `Go to ${p.label}`, hint: "Page", go: () => go(p.id) });
    out.push({ id: "setup", label: "Run setup check", hint: "Help", go: onSetup });
    out.push({ id: "theme-light", label: "Theme: Light", hint: "Look", go: () => setTheme("light") });
    out.push({ id: "theme-dark", label: "Theme: Dark", hint: "Look", go: () => setTheme("dark") });
    out.push({ id: "theme-system", label: "Theme: Follow system", hint: "Look", go: () => setTheme("system") });
    return out;
  }, [sandboxes, go, run, accel, setTheme, onSetup]);

  const shown = useMemo(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    return items.filter((i) => words.every((w) => i.label.toLowerCase().includes(w)));
  }, [items, query]);

  const index = Math.min(sel, Math.max(0, shown.length - 1));

  useEffect(() => {
    const el = listRef.current?.children[index] as HTMLElement | undefined;
    el?.scrollIntoView({ block: "nearest" });
  }, [index, shown]);

  function choose(i: Item | undefined) {
    if (!i) return;
    onClose();
    i.go();
  }

  function onKeyDown(e: ReactKeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel(shown.length === 0 ? 0 : (index + 1) % shown.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel(shown.length === 0 ? 0 : (index - 1 + shown.length) % shown.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(shown[index]);
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/50 p-4 pt-[15vh]"
      role="dialog"
      aria-modal="true"
      aria-label="Command bar"
      onClick={onClose}
    >
      <div className="app-card pop-in w-full max-w-lg overflow-hidden" onClick={(e) => e.stopPropagation()}>
        <input
          autoFocus
          className="app-input rounded-none border-0 border-b py-3 text-base"
          style={{ borderColor: "var(--border)" }}
          placeholder="Type a command, e.g. start lab"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setSel(0);
          }}
          onKeyDown={onKeyDown}
          aria-label="Command"
        />
        <ul ref={listRef} className="max-h-72 overflow-auto p-1">
          {shown.length === 0 && <li className="app-muted p-3 text-sm">Nothing matches.</li>}
          {shown.map((i, n) => (
            <li key={i.id}>
              <button
                className={`app-nav justify-between ${n === index ? "app-nav-active" : ""}`}
                onMouseEnter={() => setSel(n)}
                onClick={() => choose(i)}
              >
                <span>{i.label}</span>
                <span className="app-muted text-xs">{i.hint}</span>
              </button>
            </li>
          ))}
        </ul>
        <div className="app-muted border-t px-3 py-2 text-xs" style={{ borderColor: "var(--border)" }}>
          Up and Down to choose, Enter to run, Esc to close
        </div>
      </div>
    </div>
  );
}