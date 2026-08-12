-- SPEC-8301d106 / Issue #694: track the audit hash algorithm independently
-- from user-editable settings so the v1 -> v2 rehash runs exactly once.
CREATE TABLE audit_hash_chain_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    algorithm_version INTEGER NOT NULL
);

INSERT INTO audit_hash_chain_state (id, algorithm_version) VALUES (1, 1);
