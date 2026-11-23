export type OsImage = {
  id: string;
  name: string;
  version: string;
  image_path: string;
  image_format: string;
  checksum: string | null;
  verified: boolean;
  created_at: number;
};

export type Sandbox = {
  id: string;
  name: string;
  description: string;
  operating_system: string;
  os_version: string;
  base_image_id: string;
  overlay_path: string;
  cpu_cores: number;
  ram_mb: number;
  disk_capacity_gb: number;
  status: string;
  process_id: number | null;
  created_at: number;
  updated_at: number;
  last_started_at: number | null;
};

export type ActivityEntry = {
  id: number;
  operation: string;
  sandbox_id: string | null;
  status: string;
  message: string;
  timestamp: number;
};

export type ToolInfo = { path: string; version: string };

export type QemuDetection = {
  qemu: ToolInfo | null;
  qemu_img: ToolInfo | null;
  whpx_listed: boolean;
};

export type HostMetrics = {
  cpu_percent: number;
  cpu_threads: number;
  total_ram_mb: number;
  used_ram_mb: number;
  available_ram_mb: number;
  disk_total_gb: number;
  disk_free_gb: number;
};

export type StorageInfo = {
  data_dir: string;
  images_dir: string;
  sandboxes_dir: string;
  images_used_mb: number;
  sandboxes_used_mb: number;
  disk_free_gb: number;
};

export type SandboxMetrics = {
  id: string;
  name: string;
  status: string;
  pid: number | null;
  memory_mb: number | null;
  overlay_mb: number;
  uptime_secs: number | null;
  configured_ram_mb: number;
  configured_cpu: number;
};

export type VerifyReport = {
  matches: boolean;
  read_only: boolean;
  expected: string | null;
  actual: string;
};

export type InstallRequest = {
  name: string;
  version: string;
  isoPath: string;
  diskGb: number;
  ramMb: number;
  cpu: number;
  accel: string;
  expectedSha256: string;
};

export type InstallStarted = {
  imageId: string;
  pid: number;
  masterPath: string;
  isoSha256: string;
  isoChecksumVerified: boolean;
};

export type InstallStatus = {
  active: boolean;
  running: boolean;
  exitCode: number | null;
  imageId: string | null;
  name: string | null;
  logTail: string | null;
};

export type CreateRequest = {
  name: string;
  description: string;
  imageId: string;
  cpu: number;
  ramMb: number;
};

export type Page =
  | "dashboard"
  | "sandboxes"
  | "create"
  | "images"
  | "monitor"
  | "logs"
  | "settings"
  | "help";