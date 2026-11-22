CREATE TABLE plugin_installs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plugin TEXT NOT NULL,
    service_id UUID NOT NULL UNIQUE REFERENCES services(id) ON DELETE CASCADE,
    installed_version TEXT NOT NULL,
    image TEXT NOT NULL,
    backup_job_id UUID REFERENCES backup_jobs(id) ON DELETE SET NULL,
    status TEXT NOT NULL DEFAULT 'installed',
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE plugin_release_checks (
    plugin TEXT PRIMARY KEY,
    checked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    latest_version TEXT,
    releases JSONB NOT NULL DEFAULT '[]',
    error TEXT
);
