-- Proposal cleanup deletes negotiations in batches. SQLite checks this child
-- foreign key for each deleted negotiation, so negotiation_id must be indexed
-- as the leading column. Keep the expiration-first index for active proposals.
CREATE INDEX IF NOT EXISTS market_proposal_negotiation_id_idx
    ON market_proposal (negotiation_id);
