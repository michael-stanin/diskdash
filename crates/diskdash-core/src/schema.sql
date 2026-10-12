-- One SQLite file per snapshot. Entry ids follow DFS pre-order within a work
-- unit's id block (block << 40 | seq), so every directory's subtree is the
-- contiguous range [id, last_desc_id].

CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS entries (
    id           INTEGER PRIMARY KEY,
    parent_id    INTEGER,
    name         TEXT    NOT NULL,
    name_lower   TEXT    NOT NULL,
    kind         INTEGER NOT NULL,          -- 0 file, 1 dir, 2 link
    depth        INTEGER NOT NULL,          -- root = 0
    size         INTEGER NOT NULL,          -- logical; recursive total for dirs
    alloc        INTEGER NOT NULL,          -- allocated; recursive total for dirs
    ctime        INTEGER,                   -- unix seconds
    mtime        INTEGER,
    atime        INTEGER,
    attrs        INTEGER NOT NULL,
    reparse_tag  INTEGER NOT NULL DEFAULT 0,
    file_id      TEXT,                      -- hex, when read
    nlink        INTEGER,
    hardlink_of  INTEGER,                   -- first path of a multi-link file
    ext          TEXT,
    category     INTEGER,
    flags        INTEGER NOT NULL DEFAULT 0, -- 1 access_denied, 2 cloud_placeholder, 4 link_count_unknown
    link_target  TEXT,
    path         TEXT,                      -- directories only
    file_count   INTEGER,                   -- directories only (recursive)
    dir_count    INTEGER,
    max_mtime    INTEGER,
    max_atime    INTEGER,
    last_desc_id INTEGER,
    top_category INTEGER                    -- directories: category with the most bytes below
);

CREATE TABLE IF NOT EXISTS errors (
    id      INTEGER PRIMARY KEY,
    block   INTEGER NOT NULL DEFAULT 0,
    path    TEXT    NOT NULL,
    op      TEXT    NOT NULL,
    code    INTEGER NOT NULL,
    message TEXT    NOT NULL
);

-- Work units for cancel/resume: block 0 = files directly in the root,
-- blocks 1.. = the root's subdirectories.
CREATE TABLE IF NOT EXISTS units (
    block      INTEGER PRIMARY KEY,
    path       TEXT    NOT NULL,
    status     TEXT    NOT NULL,            -- pending | done
    size       INTEGER NOT NULL DEFAULT 0,
    alloc      INTEGER NOT NULL DEFAULT 0,
    file_count INTEGER NOT NULL DEFAULT 0,
    dir_count  INTEGER NOT NULL DEFAULT 0,
    max_mtime  INTEGER NOT NULL DEFAULT 0,
    max_atime  INTEGER NOT NULL DEFAULT 0,
    last_id    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS category_totals (
    category   INTEGER PRIMARY KEY,
    file_count INTEGER NOT NULL,
    size       INTEGER NOT NULL,
    alloc      INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ext_totals (
    ext        TEXT PRIMARY KEY,
    category   INTEGER NOT NULL,
    file_count INTEGER NOT NULL,
    size       INTEGER NOT NULL,
    alloc      INTEGER NOT NULL
);
