CREATE TABLE mcp_calls (
    id BIGSERIAL PRIMARY KEY,
    grant_id UUID REFERENCES mcp_grants(id) ON DELETE SET NULL,
    client_id TEXT,
    tool TEXT NOT NULL,
    scope TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('ok', 'denied', 'error')),
    http_status INTEGER NOT NULL CHECK (http_status BETWEEN 100 AND 599),
    duration_ms BIGINT NOT NULL CHECK (duration_ms >= 0),
    error_message TEXT CHECK (length(error_message) <= 240),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    finished_at TIMESTAMPTZ DEFAULT clock_timestamp()
);
CREATE INDEX mcp_calls_time ON mcp_calls (created_at DESC);
CREATE INDEX mcp_calls_client ON mcp_calls (client_id, created_at DESC);
CREATE INDEX mcp_calls_tool ON mcp_calls (tool, created_at DESC);
CREATE INDEX mcp_calls_status ON mcp_calls (status, created_at DESC);
