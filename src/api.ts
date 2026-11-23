import { invoke } from "@tauri-apps/api/core";
import type { Settings } from "./settings";
import type {
  ActivityEntry,
  CreateRequest,
  HostMetrics,
  InstallRequest,
  InstallStarted,
  InstallStatus,
  OsImage,
  QemuDetection,
  Sandbox,
  SandboxMetrics,
  StorageInfo,
  VerifyReport,
} from "./types";

export type SnapshotInfo = {
  id: string;
  sandbox_id: string;
  name: string;
  created_at: number;
  size_bytes: number;
};

/** Turns a backend error object into one readable line. */
export function errText(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) {
    const o = e as { message?: unknown; code?: unknown };
    const code = typeof o.code === "string" ? ` [${o.code}]` : "";
    return `${String(o.message)}${code}`;
  }
  return String(e);
}

export const api = {
  qemu: () => invoke<QemuDetection>("detect_qemu"),
  diskSelfTest: () => invoke<unknown>("run_disk_selftest"),
  images: () => invoke<OsImage[]>("list_os_images"),
  sandboxes: () => invoke<Sandbox[]>("list_sandboxes"),
  activity: (limit: number) => invoke<ActivityEntry[]>("get_activity_log", { limit }),
  host: () => invoke<HostMetrics>("get_host_metrics"),
  storage: () => invoke<StorageInfo>("get_storage_info"),
  sandboxMetrics: () => invoke<SandboxMetrics[]>("get_sandbox_metrics"),
  create: (req: CreateRequest) => invoke<Sandbox>("create_sandbox", { req }),
  start: (id: string, accel: string) => invoke<Sandbox>("start_sandbox", { id, accel }),
  stop: (id: string, force: boolean) => invoke<Sandbox>("stop_sandbox", { id, force }),
  duplicate: (id: string, newName: string) => invoke<Sandbox>("duplicate_sandbox", { id, newName }),
  setExpiry: (id: string, minutes: number | null) => invoke<number | null>("set_sandbox_expiry", { id, minutes }),
  expiry: (id: string) => invoke<number | null>("get_sandbox_expiry", { id }),
  snapshots: (id: string) => invoke<SnapshotInfo[]>("list_snapshots", { id }),
  createSnapshot: (id: string, name: string) => invoke<SnapshotInfo>("create_snapshot", { id, name }),
  restoreSnapshot: (id: string, snapshotId: string) => invoke<Sandbox>("restore_snapshot", { id, snapshotId }),
  deleteSnapshot: (id: string, snapshotId: string) => invoke<string>("delete_snapshot", { id, snapshotId }),
  reset: (id: string) => invoke<Sandbox>("reset_sandbox", { id }),
  remove: (id: string) => invoke<string>("delete_sandbox", { id }),
  vmLog: (id: string) => invoke<string>("get_vm_log", { id }),
  verify: (imageId: string) => invoke<VerifyReport>("verify_image", { imageId }),
  installStart: (req: InstallRequest) => invoke<InstallStarted>("start_master_install", { req }),
  installStatus: () => invoke<InstallStatus>("get_install_status"),
  installFinish: () => invoke<OsImage>("finish_master_install"),
  installCancel: () => invoke<void>("cancel_master_install"),
  settings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
};