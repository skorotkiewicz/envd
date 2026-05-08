ALTER TABLE envs ADD COLUMN IF NOT EXISTS vault TEXT NOT NULL DEFAULT '0';

ALTER TABLE envs DROP CONSTRAINT IF EXISTS envs_project_key_key;
ALTER TABLE envs ADD CONSTRAINT envs_project_vault_key_key UNIQUE (project, vault, key);
