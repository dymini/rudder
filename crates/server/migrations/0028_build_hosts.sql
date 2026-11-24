ALTER TABLE servers
    ADD COLUMN builder_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN builder_max_jobs INTEGER NOT NULL DEFAULT 1 CHECK (builder_max_jobs > 0),
    ADD COLUMN builder_paused BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN builder_draining BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN builder_cache_limit_bytes BIGINT NOT NULL DEFAULT 21474836480 CHECK (builder_cache_limit_bytes >= 0),
    ADD COLUMN arch TEXT,
    ADD COLUMN builder_buildx BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN builder_nixpacks BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE services
    ADD COLUMN build_placement TEXT NOT NULL DEFAULT 'panel' CHECK (build_placement IN ('panel', 'auto', 'builder')),
    ADD COLUMN build_builder_id UUID REFERENCES servers (id) ON DELETE SET NULL,
    ADD COLUMN build_fallback TEXT NOT NULL DEFAULT 'panel' CHECK (build_fallback IN ('panel', 'wait')),
    ADD COLUMN build_test_command TEXT,
    ADD COLUMN build_timeout_secs INTEGER NOT NULL DEFAULT 3600 CHECK (build_timeout_secs > 0);

ALTER TABLE build_queue ADD COLUMN builder_id UUID REFERENCES servers (id) ON DELETE SET NULL;
CREATE INDEX build_queue_builder_running_idx ON build_queue (builder_id) WHERE status = 'running';

CREATE TABLE build_cache_entries (
    builder_id UUID NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    service_id UUID NOT NULL REFERENCES services (id) ON DELETE CASCADE,
    cache_key TEXT NOT NULL CHECK (length(cache_key) BETWEEN 1 AND 128),
    size_bytes BIGINT NOT NULL CHECK (size_bytes >= 0),
    last_used_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (builder_id, service_id, cache_key)
);
CREATE INDEX build_cache_entries_lru_idx ON build_cache_entries (builder_id, last_used_at);
