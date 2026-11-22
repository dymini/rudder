ALTER TABLE cloudflare_settings
    ADD COLUMN cloudflared_version TEXT,
    ADD COLUMN cloudflared_release JSONB,
    ADD COLUMN cloudflared_checked_at TIMESTAMPTZ,
    ADD COLUMN cloudflared_check_error TEXT,
    ADD COLUMN cloudflared_update JSONB;
