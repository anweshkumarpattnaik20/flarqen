-- Repair: make sure the auto-delete and snapshot tables exist.
CREATE TABLE IF NOT EXISTS sandbox_expiry (
  sandbox_id TEXT PRIMARY KEY NOT NULL REFERENCES sandboxes(id) ON DELETE CASCADE,
  expires_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sandbox_expiry_at ON sandbox_expiry(expires_at);

CREATE TABLE IF NOT EXISTS sandbox_snapshots (
  id TEXT PRIMARY KEY NOT NULL,
  sandbox_id TEXT NOT NULL REFERENCES sandboxes(id) ON DELETE CASCADE,
  name TEXT NOT NULL COLLATE NOCASE,
  created_at INTEGER NOT NULL,
  size_bytes INTEGER NOT NULL DEFAULT 0,
  UNIQUE(sandbox_id, name)
);

CREATE INDEX IF NOT EXISTS idx_sandbox_snapshots_sb ON sandbox_snapshots(sandbox_id);