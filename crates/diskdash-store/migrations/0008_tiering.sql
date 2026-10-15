-- 0008_tiering: which tiering suggestions the user marked as planned (for the
-- projected free-space view). Keyed by the lower-case source path, so it
-- survives a rescan. Nothing here moves anything.

CREATE TABLE tiering_planned (
    path_key   TEXT PRIMARY KEY,
    created_at TEXT NOT NULL
);
