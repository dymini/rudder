ALTER TABLE domains ADD COLUMN routing_mode TEXT NOT NULL DEFAULT 'direct'
    CHECK (routing_mode IN ('direct', 'cloudflare'));

UPDATE domains d SET routing_mode = 'cloudflare'
FROM services s JOIN servers sv ON sv.id = s.server_id
WHERE d.service_id = s.id AND sv.kind = 'local'
  AND EXISTS (SELECT 1 FROM cloudflare_settings WHERE tunnel_id IS NOT NULL AND tunnel_published);

CREATE TABLE domain_dns_records (
    zone_id TEXT NOT NULL,
    host TEXT NOT NULL,
    record_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    content TEXT NOT NULL,
    PRIMARY KEY (zone_id, record_id)
);
