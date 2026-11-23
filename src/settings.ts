export type Settings = {
  qemu_path: string | null;
  qemu_img_path: string | null;
  default_cpu: number;
  default_ram_mb: number;
  acceleration: string;
  log_retention_days: number;
};

export const DEFAULT_SETTINGS: Settings = {
  qemu_path: null,
  qemu_img_path: null,
  default_cpu: 2,
  default_ram_mb: 2048,
  acceleration: "whpx",
  log_retention_days: 90,
};