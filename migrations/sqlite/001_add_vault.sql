ALTER TABLE envs RENAME TO envs_old;

CREATE TABLE envs (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    project TEXT NOT NULL REFERENCES projects(name) ON DELETE CASCADE,
    vault   TEXT NOT NULL DEFAULT '0',
    key     TEXT NOT NULL,
    value   TEXT NOT NULL,
    updated TEXT DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(project, vault, key)
);

INSERT INTO envs (project, vault, key, value, updated)
SELECT project, '0', key, value, updated FROM envs_old;

DROP TABLE envs_old;
