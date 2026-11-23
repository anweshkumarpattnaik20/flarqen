-- Named copies of a sandbox's disk. Rows disappear with their sandbox.
CREATE TABLE sandbox_snapshots (
  id TEXT PRIMARY KEY NOT NULL,
  sandbox_id TEXT NOT NULL REFERENCES sandboxes(id) ON DELETE CASCADE,
  name TEXT NOT NULL COLLATE NOCASE,
  created_at INTEGER NOT NULL,
  size_bytes INTEGER NOT NULL DEFAULT 0,
  UNIQUE (sandbox_id, name)
);

CREATE INDEX idx_snapshots_sandbox ON sandbox_snapshots(sandbox_id);