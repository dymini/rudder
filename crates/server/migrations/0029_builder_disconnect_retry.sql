ALTER TABLE build_queue
    ADD COLUMN disconnect_retries SMALLINT NOT NULL DEFAULT 0
        CHECK (disconnect_retries BETWEEN 0 AND 1);
