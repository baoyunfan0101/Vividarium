CREATE TABLE storage_settings (
    settings_id INTEGER PRIMARY KEY CHECK (settings_id = 1),
    taxonomy_db_path TEXT NOT NULL,
    default_taxonomy_directory TEXT NOT NULL,
    default_photo_library_directory TEXT NOT NULL
);

CREATE TABLE photo_libraries (
    library_uuid TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    root_path TEXT NOT NULL UNIQUE,
    db_path TEXT NOT NULL UNIQUE,
    last_opened_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (length(library_uuid) > 0),
    CHECK (length(display_name) > 0),
    CHECK (length(root_path) > 0)
);

CREATE TABLE taxonomy_sync_dispatch (
    dispatch_id INTEGER PRIMARY KEY CHECK (dispatch_id = 1),
    last_dispatched_sync_id INTEGER NOT NULL DEFAULT 0,
    CHECK (last_dispatched_sync_id >= 0)
);

INSERT INTO taxonomy_sync_dispatch (dispatch_id, last_dispatched_sync_id)
VALUES (1, 0);

CREATE TABLE photo_library_taxonomy_pending (
    library_uuid TEXT PRIMARY KEY,
    target_sync_id INTEGER NOT NULL,
    full_remap_required INTEGER NOT NULL DEFAULT 0,
    CHECK (target_sync_id >= 0),
    CHECK (full_remap_required IN (0, 1)),
    FOREIGN KEY (library_uuid)
        REFERENCES photo_libraries(library_uuid) ON DELETE CASCADE
);

CREATE TABLE photo_library_taxonomy_pending_taxa (
    library_uuid TEXT NOT NULL,
    taxon_id INTEGER NOT NULL,
    PRIMARY KEY (library_uuid, taxon_id),
    FOREIGN KEY (library_uuid)
        REFERENCES photo_library_taxonomy_pending(library_uuid) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE TABLE active_photo_library (
    active_id INTEGER PRIMARY KEY CHECK (active_id = 1),
    library_uuid TEXT NOT NULL UNIQUE,
    FOREIGN KEY (library_uuid)
        REFERENCES photo_libraries(library_uuid) ON DELETE RESTRICT
);

CREATE TABLE app_metadata (
    metadata_key TEXT PRIMARY KEY,
    metadata_value TEXT NOT NULL
);

CREATE TABLE sql_inputs (
    scope INTEGER NOT NULL,
    alias TEXT COLLATE NOCASE NOT NULL,
    source_type INTEGER NOT NULL,
    original_path TEXT NOT NULL,
    stored_path TEXT NOT NULL UNIQUE,
    schema_json TEXT NOT NULL,
    PRIMARY KEY (scope, alias),
    CHECK (scope IN (1, 2)),
    CHECK (source_type IN (1, 2)),
    CHECK (length(alias) > 0),
    CHECK (length(stored_path) > 0)
) WITHOUT ROWID;

PRAGMA user_version = 2;
