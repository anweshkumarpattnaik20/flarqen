-- Optional auto-delete time for a sandbox. The row disappears with the sandbox.
CREATE TABLE sandbox_expiry (
  sandbox_id TEXT PRIMARY KEY NOT NULL REFERENCES sandboxes(id) ON DELETE CASCADE,
  expires_at INTEGER NOT NULL
);

CREATE INDEX idx_sandbox_expiry_at ON sandbox_expiry(expires_at);