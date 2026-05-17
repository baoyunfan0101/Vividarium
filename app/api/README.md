# API Module

This module exposes the current `photos`, `taxa`, and `photos_taxa_mapping` services through FastAPI.

## Run

```bash
.venv/bin/uvicorn app.api.main:app --reload --host 127.0.0.1 --port 8000
```

Interactive API docs are available at:

```text
http://127.0.0.1:8000/docs
```

## Confirmation Flow

Some operations intentionally return a confirmation response instead of mutating the database. The frontend should show the message to the user and, if the user confirms, call the same endpoint again with `force: true`.

Confirmation response shape:

```json
{
  "needs_confirmation": true,
  "reason": "knowledge_base_unchanged",
  "message": "Knowledge-base file size and modified time are unchanged."
}
```

Current confirmation reasons:

- `knowledge_base_unchanged`: returned by `POST /taxa/update` and `POST /taxa/rebuild` when the selected knowledge-base file has the same path, size, and modified time as the stored metadata.
- `taxa_newer_than_photos`: returned by `POST /mapping/photos-taxa/update` when the taxa module was synced later than the photos module.

## Photos Routes

- `GET /photos/roots`: return recorded photo roots and root metadata.
- `PUT /photos/roots`: replace the stored root list and order without scanning files. Body: `{"roots": ["..."]}`.
- `GET /photos/browse?root=...&relative_dir=...`: browse directories and files under a root.
- `GET /photos/all`: return the full `photos` table.
- `GET /photos/changed`: return photos with `updated` or `new` status.
- `GET /photos/latest-update`: return `photos_metadata`.
- `GET /photos/file/{photo_id}`: return the original photo file for preview.
- `GET /photos/{photo_id}`: return one photo row.
- `POST /photos/update`: update one or more roots. Body: `{"root": "..."}` or `{"roots": ["..."]}`. If omitted, the API uses the only recorded root when exactly one exists.
- `POST /photos/rebuild`: rebuild rows for the specified roots. Body: `{"roots": ["..."]}`. If omitted, the API uses recorded roots.
- `GET /photos/export?table_name=...`: download `photos` or `photos_metadata` as CSV.
- `POST /photos/export`: export `photos` or `photos_metadata` to a backend filesystem path.

## Taxa Routes

- `GET /taxa/latest-update`: return `taxa_metadata`.
- `PUT /taxa/knowledge-base`: save the configured knowledge-base path without importing taxa. Body: `{"knowledge_base_path": "..."}`.
- `GET /taxa/by-id/{taxon_id}`: return one taxa row by id.
- `GET /taxa/by-binomial?binomial_name=...`: return one taxa row by binomial name.
- `POST /taxa/update`: update taxa. Body: `{"knowledge_base_path": "...", "force": false}`. Path may be omitted when metadata already stores one.
- `POST /taxa/rebuild`: rebuild taxa. Body: `{"knowledge_base_path": "...", "force": false}`. Path may be omitted when metadata already stores one.
- `GET /taxa/export?table_name=...`: download `taxa` or `taxa_metadata` as CSV.
- `POST /taxa/export`: export `taxa` or `taxa_metadata` to a backend filesystem path.

## Photos-Taxa Mapping Routes

- `GET /mapping/photos-taxa/latest-update`: return mapping metadata.
- `GET /mapping/photos-taxa/root`: return top-level cached taxa nodes and photo ids.
- `GET /mapping/photos-taxa/taxon/{taxon_id}`: return exact photo ids and child taxa rows.
- `GET /mapping/photos-taxa/search-binomial?binomial_name=...`: same as above, found by binomial name.
- `GET /mapping/photos-taxa/search?name=...`: same as above, found by display name.
- `POST /mapping/photos-taxa/update`: update changed photos only. Body: `{"force": false}`.
- `POST /mapping/photos-taxa/rebuild`: rebuild from all photos. Body: `{"force": false}`.
- `GET /mapping/photos-taxa/export?table_name=...`: download a mapping table as CSV.
- `POST /mapping/photos-taxa/export`: export a mapping table to a backend filesystem path.
