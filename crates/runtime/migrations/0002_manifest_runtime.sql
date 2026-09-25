CREATE TABLE IF NOT EXISTS ppsc_manifest_state (
    contract_id BYTEA NOT NULL CHECK (octet_length(contract_id) = 32),
    state_name TEXT NOT NULL,
    state_key BYTEA NOT NULL,
    value_type SMALLINT NOT NULL,
    payload BYTEA NOT NULL,
    version BIGINT NOT NULL DEFAULT 1 CHECK (version > 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (contract_id, state_name, state_key)
);

CREATE TABLE IF NOT EXISTS ppsc_manifest_inputs (
    data_id BYTEA PRIMARY KEY CHECK (octet_length(data_id) = 32),
    value_type SMALLINT NOT NULL,
    payload BYTEA NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ppsc_manifest_daemon_cursors (
    control_address TEXT NOT NULL,
    node_id TEXT NOT NULL,
    next_execution_index BIGINT NOT NULL DEFAULT 0 CHECK (next_execution_index >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (control_address, node_id)
);

CREATE TABLE IF NOT EXISTS ppsc_manifest_processed_tasks (
    control_address TEXT NOT NULL,
    node_id TEXT NOT NULL,
    queue_index BIGINT NOT NULL CHECK (queue_index >= 0),
    execution_id BYTEA NOT NULL CHECK (octet_length(execution_id) = 32),
    outcome TEXT NOT NULL CHECK (outcome IN ('completed', 'skipped', 'rejected')),
    processed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (control_address, node_id, queue_index),
    UNIQUE (control_address, node_id, execution_id)
);

CREATE TABLE IF NOT EXISTS ppsc_manifest_result_outbox (
    control_address TEXT NOT NULL,
    node_id TEXT NOT NULL,
    execution_id BYTEA NOT NULL CHECK (octet_length(execution_id) = 32),
    result_payload BYTEA NOT NULL,
    submitted BOOLEAN NOT NULL DEFAULT FALSE,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (control_address, node_id, execution_id)
);

-- Authenticated user uploads are committed locally before the runtime submits
-- their metadata on chain.  This outbox makes registration retryable across
-- RPC failures and daemon restarts without ever requiring the user's key.
CREATE TABLE IF NOT EXISTS ppsc_manifest_uploads (
    control_address TEXT NOT NULL,
    contract_id BYTEA NOT NULL CHECK (octet_length(contract_id) = 32),
    data_id BYTEA NOT NULL CHECK (octet_length(data_id) = 32),
    owner BYTEA NOT NULL CHECK (octet_length(owner) = 20),
    upload_nonce BIGINT NOT NULL CHECK (upload_nonce >= 0),
    deadline BIGINT NOT NULL CHECK (deadline >= 0),
    value_type SMALLINT NOT NULL,
    commitment BYTEA NOT NULL CHECK (octet_length(commitment) = 32),
    registered BOOLEAN NOT NULL DEFAULT FALSE,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (control_address, contract_id, data_id),
    UNIQUE (control_address, contract_id, owner, upload_nonce)
);
