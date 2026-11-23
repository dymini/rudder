CREATE TABLE cloudflare_zones (
    zone_id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    accessible BOOLEAN NOT NULL DEFAULT TRUE,
    last_error TEXT,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_hosts TEXT[] NOT NULL DEFAULT '{}',
    UNIQUE(account_id, name)
);

INSERT INTO cloudflare_zones(zone_id, account_id, name, enabled, published_hosts)
SELECT zone_id, account_id, lower(zone_name), TRUE, published_hosts
FROM cloudflare_settings WHERE zone_id <> '' AND zone_name <> '';
