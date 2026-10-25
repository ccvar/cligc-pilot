-- Cligc Pilot 动态功能的数据表（D1）。可以重复执行。
CREATE TABLE IF NOT EXISTS submissions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  form TEXT NOT NULL,
  page TEXT,
  fields TEXT NOT NULL,
  ip TEXT,
  ua TEXT,
  created_at INTEGER NOT NULL,
  read INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_submissions_created ON submissions (created_at);
CREATE INDEX IF NOT EXISTS idx_submissions_ip ON submissions (ip, created_at);
CREATE TABLE IF NOT EXISTS comments (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  path TEXT NOT NULL,
  parent_id INTEGER,
  name TEXT NOT NULL,
  email TEXT,
  url TEXT,
  content TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  ip TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_comments_path ON comments (path, status, created_at);
CREATE INDEX IF NOT EXISTS idx_comments_ip ON comments (ip, created_at);
