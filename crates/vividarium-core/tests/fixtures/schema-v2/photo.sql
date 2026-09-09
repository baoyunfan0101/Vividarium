CREATE TABLE photo_library (
    library_id INTEGER PRIMARY KEY CHECK (library_id = 1),
    library_uuid TEXT NOT NULL UNIQUE,
    root_path TEXT NOT NULL,
    bound_taxonomy_identity TEXT NOT NULL,
    last_taxonomy_sync_id INTEGER NOT NULL DEFAULT 0,
    CHECK (last_taxonomy_sync_id >= 0)
);

CREATE TABLE photo_directories (
    directory_id INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_directory_id INTEGER,
    name TEXT NOT NULL,
    relative_path TEXT NOT NULL UNIQUE,
    UNIQUE (parent_directory_id, name),
    FOREIGN KEY (parent_directory_id)
        REFERENCES photo_directories(directory_id) ON DELETE CASCADE
);

CREATE TABLE photos (
    photo_id INTEGER PRIMARY KEY AUTOINCREMENT,
    directory_id INTEGER NOT NULL,
    filename TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    modified_at_ns INTEGER NOT NULL,
    thumbnail_path TEXT,
    UNIQUE (directory_id, filename),
    CHECK (length(filename) > 0),
    CHECK (file_size >= 0),
    FOREIGN KEY (directory_id)
        REFERENCES photo_directories(directory_id) ON DELETE CASCADE
);

CREATE TABLE photo_metadata (
    photo_id INTEGER PRIMARY KEY,
    captured_at TEXT,
    camera TEXT,
    width INTEGER,
    height INTEGER,
    longitude REAL,
    latitude REAL,
    exif_json TEXT,
    FOREIGN KEY (photo_id) REFERENCES photos(photo_id) ON DELETE CASCADE
);

CREATE VIRTUAL TABLE photo_filenames_fts USING fts5(
    filename,
    content = 'photos',
    content_rowid = 'photo_id',
    tokenize = 'trigram'
);

CREATE TRIGGER photos_ai AFTER INSERT ON photos BEGIN
    INSERT INTO photo_filenames_fts(rowid, filename)
    VALUES (new.photo_id, new.filename);
END;

CREATE TRIGGER photos_ad AFTER DELETE ON photos BEGIN
    INSERT INTO photo_filenames_fts(photo_filenames_fts, rowid, filename)
    VALUES ('delete', old.photo_id, old.filename);
END;

CREATE TRIGGER photos_au AFTER UPDATE OF filename ON photos BEGIN
    INSERT INTO photo_filenames_fts(photo_filenames_fts, rowid, filename)
    VALUES ('delete', old.photo_id, old.filename);
    INSERT INTO photo_filenames_fts(rowid, filename)
    VALUES (new.photo_id, new.filename);
END;

CREATE TABLE photo_taxon_mapping (
    photo_id INTEGER PRIMARY KEY,
    taxon_id INTEGER,
    status TEXT NOT NULL,
    CHECK (status IN ('matched', 'unmatched', 'ambiguous')),
    CHECK ((status = 'matched' AND taxon_id IS NOT NULL)
        OR (status != 'matched' AND taxon_id IS NULL)),
    FOREIGN KEY (photo_id) REFERENCES photos(photo_id) ON DELETE CASCADE
);

CREATE TABLE photo_taxon_candidates (
    photo_id INTEGER NOT NULL,
    taxon_id INTEGER NOT NULL,
    PRIMARY KEY (photo_id, taxon_id),
    FOREIGN KEY (photo_id)
        REFERENCES photo_taxon_mapping(photo_id) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE TABLE photo_taxon_candidate_names (
    photo_id INTEGER NOT NULL,
    taxon_id INTEGER NOT NULL,
    name_id INTEGER NOT NULL,
    name_type INTEGER NOT NULL,
    name TEXT NOT NULL,
    PRIMARY KEY (photo_id, taxon_id, name_id),
    CHECK (name_type BETWEEN 1 AND 6),
    CHECK (length(trim(name)) > 0),
    FOREIGN KEY (photo_id, taxon_id)
        REFERENCES photo_taxon_candidates(photo_id, taxon_id) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE TRIGGER photo_taxon_candidates_bi
BEFORE INSERT ON photo_taxon_candidates
WHEN NOT EXISTS (
    SELECT 1
    FROM photo_taxon_mapping
    WHERE photo_id = new.photo_id
      AND status = 'ambiguous'
) BEGIN
    SELECT RAISE(ABORT, 'photo candidates require an ambiguous mapping');
END;

CREATE TRIGGER photo_taxon_mapping_au_candidates
AFTER UPDATE OF status ON photo_taxon_mapping
WHEN new.status != 'ambiguous' BEGIN
    DELETE FROM photo_taxon_candidates WHERE photo_id = new.photo_id;
END;

CREATE TABLE photo_taxon_usage (
    taxon_id INTEGER PRIMARY KEY,
    direct_photo_count INTEGER NOT NULL,
    subtree_photo_count INTEGER NOT NULL,
    CHECK (direct_photo_count >= 0),
    CHECK (subtree_photo_count >= direct_photo_count)
);

CREATE TABLE photo_mapping_queue (
    photo_id INTEGER PRIMARY KEY,
    reason TEXT NOT NULL,
    CHECK (reason IN ('refresh', 'taxonomy', 'hook', 'settings')),
    FOREIGN KEY (photo_id) REFERENCES photos(photo_id) ON DELETE CASCADE
);

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
    FOREIGN KEY (operation_id)
        REFERENCES operations(operation_id) ON DELETE CASCADE
);

CREATE INDEX idx_photo_directories_parent_name
    ON photo_directories(parent_directory_id, name, directory_id);
CREATE INDEX idx_photos_directory_filename
    ON photos(directory_id, filename, photo_id);
CREATE INDEX idx_photo_metadata_coordinates
    ON photo_metadata(latitude, longitude, photo_id);
CREATE INDEX idx_photo_taxon_mapping_taxon
    ON photo_taxon_mapping(taxon_id, photo_id);
CREATE INDEX idx_photo_taxon_mapping_status
    ON photo_taxon_mapping(status, photo_id);
CREATE INDEX idx_photo_taxon_candidates_taxon
    ON photo_taxon_candidates(taxon_id, photo_id);
CREATE INDEX idx_photo_taxon_usage_subtree
    ON photo_taxon_usage(subtree_photo_count, taxon_id);
CREATE INDEX idx_photo_mapping_queue_reason
    ON photo_mapping_queue(reason, photo_id);
CREATE INDEX idx_operation_audit_entity
    ON operation_audit_rows(entity_type, entity_id, operation_id, sequence);
CREATE INDEX idx_operations_applied
    ON operations(applied_at DESC, operation_id DESC);

PRAGMA user_version = 2;
