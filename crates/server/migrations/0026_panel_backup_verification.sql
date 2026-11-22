CREATE TABLE panel_backup_verifications (
    run_id UUID NOT NULL REFERENCES backup_runs(id) ON DELETE CASCADE,
    destination_id UUID NOT NULL REFERENCES s3_destinations(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK(status IN ('running','succeeded','failed')),
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    manifest JSONB,
    error TEXT,
    PRIMARY KEY(run_id,destination_id)
);
CREATE TABLE panel_backup_health_state (
    id BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK(id),
    warning TEXT,
    alerted_at TIMESTAMPTZ
);
INSERT INTO panel_backup_health_state(id) VALUES(TRUE);
