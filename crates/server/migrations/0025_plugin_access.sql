ALTER TABLE access_policies ADD COLUMN bypass_paths TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE plugin_installs ADD COLUMN access_enabled BOOLEAN NOT NULL DEFAULT true;
ALTER TABLE plugin_installs ADD COLUMN safety_run_id UUID REFERENCES backup_runs(id) ON DELETE SET NULL;
