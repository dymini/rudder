ALTER TABLE mcp_grants ADD COLUMN allowed_tools TEXT[];
ALTER TABLE api_tokens
    ADD COLUMN mcp_scopes TEXT[],
    ADD COLUMN mcp_allowed_tools TEXT[],
    ADD COLUMN created_by_mcp_grant UUID REFERENCES mcp_grants(id) ON DELETE CASCADE,
    ADD COLUMN created_by_mcp_client TEXT;
