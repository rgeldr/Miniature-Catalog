CREATE TABLE IF NOT EXISTS locations (
    id INTEGER PRIMARY KEY,
    parent_id INTEGER REFERENCES locations(id),
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    CHECK (length(trim(name)) > 0),
    CHECK (length(trim(kind)) > 0)
);

CREATE UNIQUE INDEX IF NOT EXISTS locations_root_name
    ON locations(name) WHERE parent_id IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS locations_sibling_name
    ON locations(parent_id, name) WHERE parent_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS miniatures (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    notes TEXT NOT NULL DEFAULT '',
    quantity INTEGER NOT NULL CHECK (quantity >= 0 AND quantity <= 100000),
    size TEXT NOT NULL,
    game TEXT,
    location_id INTEGER NOT NULL REFERENCES locations(id),
    thumbnail_file TEXT,
    CHECK (length(trim(name)) > 0),
    CHECK (length(trim(size)) > 0),
    CHECK (game IS NULL OR length(trim(game)) > 0)
);

CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY,
    category TEXT NOT NULL CHECK (category IN ('race', 'combat', 'role')),
    name TEXT NOT NULL,
    UNIQUE (category, name)
);

CREATE TABLE IF NOT EXISTS miniature_tags (
    miniature_id INTEGER NOT NULL REFERENCES miniatures(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id),
    PRIMARY KEY (miniature_id, tag_id)
);

CREATE TABLE IF NOT EXISTS api_keys (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    scope TEXT NOT NULL CHECK (scope IN ('read', 'admin')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    revoked_at TEXT,
    CHECK (length(trim(name)) > 0)
);

CREATE INDEX IF NOT EXISTS idx_miniatures_location ON miniatures(location_id);
CREATE INDEX IF NOT EXISTS idx_miniature_tags_tag ON miniature_tags(tag_id);
