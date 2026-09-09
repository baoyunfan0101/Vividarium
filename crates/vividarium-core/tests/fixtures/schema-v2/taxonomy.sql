CREATE TABLE taxonomy_identity (
    identity_id INTEGER PRIMARY KEY CHECK (identity_id = 1),
    taxonomy_identity TEXT NOT NULL UNIQUE
);

CREATE TABLE taxa (
    taxon_id INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_taxon_id INTEGER,
    rank INTEGER NOT NULL,
    geological_range TEXT,
    CHECK (rank IN (1, 2, 3, 4, 5)),
    FOREIGN KEY (parent_taxon_id) REFERENCES taxa(taxon_id) ON DELETE RESTRICT
);

INSERT INTO sqlite_sequence(name, seq)
SELECT 'taxa', 8000000000000000
WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'taxa');
UPDATE sqlite_sequence
SET seq = max(seq, 8000000000000000)
WHERE name = 'taxa';

CREATE TABLE taxon_names (
    name_id INTEGER PRIMARY KEY AUTOINCREMENT,
    taxon_id INTEGER NOT NULL,
    name_type INTEGER NOT NULL,
    name TEXT NOT NULL,
    normalized_name TEXT GENERATED ALWAYS AS (lower(name)) STORED,
    authority_year TEXT,
    source TEXT,
    UNIQUE (taxon_id, name_type, name),
    CHECK (name_type BETWEEN 1 AND 6),
    CHECK (length(trim(name)) > 0),
    FOREIGN KEY (taxon_id) REFERENCES taxa(taxon_id) ON DELETE CASCADE
);

CREATE VIRTUAL TABLE taxon_names_fts USING fts5(
    name,
    content = 'taxon_names',
    content_rowid = 'name_id',
    tokenize = 'trigram'
);

CREATE TRIGGER taxon_names_ai AFTER INSERT ON taxon_names BEGIN
    INSERT INTO taxon_names_fts(rowid, name) VALUES (new.name_id, new.name);
END;

CREATE TRIGGER taxon_names_ad AFTER DELETE ON taxon_names BEGIN
    INSERT INTO taxon_names_fts(taxon_names_fts, rowid, name)
    VALUES ('delete', old.name_id, old.name);
END;

CREATE TRIGGER taxon_names_au AFTER UPDATE OF name ON taxon_names BEGIN
    INSERT INTO taxon_names_fts(taxon_names_fts, rowid, name)
    VALUES ('delete', old.name_id, old.name);
    INSERT INTO taxon_names_fts(rowid, name) VALUES (new.name_id, new.name);
END;

CREATE TABLE operations (
    operation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    source TEXT NOT NULL,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    total_items INTEGER NOT NULL,
    succeeded_items INTEGER NOT NULL,
    failed_items INTEGER NOT NULL,
    rollbackable INTEGER NOT NULL,
    has_formatted_input INTEGER NOT NULL,
    CHECK (total_items >= 0),
    CHECK (succeeded_items >= 0),
    CHECK (failed_items >= 0),
    CHECK (succeeded_items + failed_items = total_items),
    CHECK (rollbackable IN (0, 1)),
    CHECK (has_formatted_input IN (0, 1))
);

CREATE TABLE operation_audit_rows (
    operation_id INTEGER NOT NULL,
    sequence INTEGER NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT,
    action TEXT NOT NULL,
    before_json TEXT,
    after_json TEXT,
    succeeded INTEGER NOT NULL,
    message TEXT NOT NULL,
    PRIMARY KEY (operation_id, sequence),
    CHECK (sequence > 0),
    CHECK (succeeded IN (0, 1)),
    FOREIGN KEY (operation_id) REFERENCES operations(operation_id) ON DELETE CASCADE
);

CREATE TABLE operation_changesets (
    operation_id INTEGER PRIMARY KEY,
    changeset_blob BLOB NOT NULL,
    FOREIGN KEY (operation_id) REFERENCES operations(operation_id) ON DELETE CASCADE
);

CREATE TABLE operation_formatted_inputs (
    operation_id INTEGER NOT NULL,
    sequence INTEGER NOT NULL,
    input_json TEXT NOT NULL,
    PRIMARY KEY (operation_id, sequence),
    CHECK (sequence > 0),
    FOREIGN KEY (operation_id) REFERENCES operations(operation_id) ON DELETE CASCADE
);

CREATE TABLE taxonomy_base_metadata (
    metadata_id INTEGER PRIMARY KEY CHECK (metadata_id = 1),
    source_path TEXT NOT NULL,
    taxa_count INTEGER NOT NULL,
    taxon_names_count INTEGER NOT NULL,
    imported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (taxa_count >= 0),
    CHECK (taxon_names_count >= 0)
);

CREATE TABLE taxonomy_sync_events (
    sync_id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_operation_id INTEGER,
    full_remap_required INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (full_remap_required IN (0, 1))
);

CREATE TABLE taxonomy_sync_event_taxa (
    sync_id INTEGER NOT NULL,
    taxon_id INTEGER NOT NULL,
    PRIMARY KEY (sync_id, taxon_id),
    FOREIGN KEY (sync_id)
        REFERENCES taxonomy_sync_events(sync_id) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE UNIQUE INDEX idx_taxon_names_one_sci_name
    ON taxon_names(taxon_id) WHERE name_type = 1;
CREATE UNIQUE INDEX idx_taxon_names_one_zh_name
    ON taxon_names(taxon_id) WHERE name_type = 3;
CREATE UNIQUE INDEX idx_taxon_names_one_en_name
    ON taxon_names(taxon_id) WHERE name_type = 5;
CREATE INDEX idx_taxa_parent ON taxa(parent_taxon_id);
CREATE INDEX idx_taxa_parent_rank_id ON taxa(parent_taxon_id, rank, taxon_id);
CREATE INDEX idx_taxa_rank ON taxa(rank);
CREATE INDEX idx_taxon_names_type_name ON taxon_names(name_type, name);
CREATE INDEX idx_taxon_names_type_taxon ON taxon_names(name_type, taxon_id);
CREATE INDEX idx_taxon_names_name ON taxon_names(name);
CREATE INDEX idx_taxon_names_name_search
    ON taxon_names(normalized_name, taxon_id);
CREATE INDEX idx_operations_applied
    ON operations(applied_at DESC, operation_id DESC);
CREATE INDEX idx_operation_audit_entity
    ON operation_audit_rows(entity_type, entity_id, operation_id, sequence);
CREATE INDEX idx_taxonomy_sync_events_created
    ON taxonomy_sync_events(sync_id);

PRAGMA user_version = 2;
