CREATE TABLE os_images (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  version TEXT NOT NULL,
  image_path TEXT NOT NULL UNIQUE,
  image_format TEXT NOT NULL,
  checksum TEXT,
  verified INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);

CREATE TABLE sandboxes (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE COLLATE NOCASE,
  description TEXT NOT NULL DEFAULT '',
  operating_system TEXT NOT NULL,
  os_version TEXT NOT NULL,
  base_image_id TEXT NOT NULL REFERENCES os_images(id),
  overlay_path TEXT NOT NULL UNIQUE,
  cpu_cores INTEGER NOT NULL CHECK (cpu_cores BETWEEN 1 AND 64),
  ram_mb INTEGER NOT NULL CHECK (ram_mb >= 256),
  disk_capacity_gb INTEGER NOT NULL CHECK (disk_capacity_gb >= 1),
  status TEXT NOT NULL,
  process_id INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  last_started_at INTEGER
);

CREATE TABLE activity_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  operation TEXT NOT NULL,
  sandbox_id TEXT,
  status TEXT NOT NULL,
  message TEXT NOT NULL,
  timestamp INTEGER NOT NULL
);