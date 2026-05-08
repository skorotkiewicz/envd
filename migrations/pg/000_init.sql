CREATE TABLE IF NOT EXISTS projects (
    id      SERIAL PRIMARY KEY,
    name    TEXT UNIQUE NOT NULL,
    created TIMESTAMPTZ DEFAULT now()
);

CREATE TABLE IF NOT EXISTS envs (
    id      SERIAL PRIMARY KEY,
    project TEXT NOT NULL REFERENCES projects(name) ON DELETE CASCADE,
    key     TEXT NOT NULL,
    value   TEXT NOT NULL,
    updated TIMESTAMPTZ DEFAULT now(),
    UNIQUE(project, key)
);
