ALTER TABLE users ADD COLUMN email_mfa_enabled BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE user_passkeys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    credential_id BYTEA NOT NULL UNIQUE,
    credential JSONB NOT NULL,
    counter BIGINT NOT NULL DEFAULT 0 CHECK (counter >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ
);
CREATE INDEX user_passkeys_user_idx ON user_passkeys(user_id);

CREATE TABLE mfa_challenges (
    token_hash TEXT PRIMARY KEY,
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    purpose TEXT NOT NULL,
    state_enc BYTEA NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL DEFAULT now() + interval '5 minutes'
);
CREATE INDEX mfa_challenges_expiry_idx ON mfa_challenges(expires_at);

CREATE TABLE mfa_email_codes (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    purpose TEXT NOT NULL CHECK (purpose IN ('enable','login','reauth')),
    code_hash TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT now() + interval '10 minutes',
    PRIMARY KEY (user_id,purpose)
);
