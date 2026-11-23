import { createContext, useContext } from "react";
import type { Settings } from "./settings";
import type { OsImage, Page, QemuDetection, Sandbox } from "./types";

export type Theme = "system" | "light" | "dark";
export type Accel = "whpx" | "tcg";

export type ConfirmOpts = {
  title: string;
  body: string;
  label: string;
  run: () => void;
};

export type AppCtx = {
  images: OsImage[];
  sandboxes: Sandbox[];
  qemu: QemuDetection | null;
  settings: Settings;
  refresh: () => Promise<void>;
  refreshQemu: () => Promise<void>;
  /** Saves settings through the backend, which validates them. Returns true when they were accepted. */
  saveSettings: (s: Settings) => Promise<boolean>;
  /** Runs a backend call, tracks busy state per sandbox ("global" when no id), and reports the outcome. */
  run: <T>(label: string, fn: () => Promise<T>, id?: string) => Promise<T | undefined>;
  busyIds: string[];
  ask: (o: ConfirmOpts) => void;
  go: (p: Page) => void;
  notify: (kind: "success" | "error" | "info", text: string) => void;
  theme: Theme;
  setTheme: (t: Theme) => void;
  /** Acceleration used when starting VMs; comes from the saved settings. */
  accel: Accel;
};

export const AppContext = createContext<AppCtx | null>(null);

export function useApp(): AppCtx {
  const c = useContext(AppContext);
  if (!c) throw new Error("App context is missing");
  return c;
}