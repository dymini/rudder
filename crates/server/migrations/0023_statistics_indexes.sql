CREATE INDEX metrics_samples_time_idx ON metrics_samples (sampled_at);
CREATE INDEX metrics_rollups_time_idx ON metrics_rollups (resolution, bucket_start);
CREATE INDEX deployments_created_idx ON deployments (created_at);
CREATE INDEX backup_runs_started_idx ON backup_runs (started_at) WHERE kind = 'backup';
CREATE INDEX backup_runs_last_success_idx ON backup_runs (job_id, finished_at DESC)
    WHERE kind = 'backup' AND status = 'succeeded';
