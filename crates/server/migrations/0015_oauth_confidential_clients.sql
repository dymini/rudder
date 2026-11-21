ALTER TABLE oauth_clients
    ADD COLUMN token_endpoint_auth_method text NOT NULL DEFAULT 'none'
        CHECK (token_endpoint_auth_method IN ('none', 'client_secret_post', 'client_secret_basic')),
    ADD COLUMN client_secret_hash text;
