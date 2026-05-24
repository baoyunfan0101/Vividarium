# API

FastAPI routes for photos, taxa, mapping, local path picking, and operation progress.

## Run

From the repository root, `python main.py` starts both backend and frontend.

Backend only:

```bash
.venv/bin/uvicorn app.api.main:app --reload --host 127.0.0.1 --port 8000
```

Docs: `http://127.0.0.1:8000/docs`.

## Confirmation Responses

Some mutating endpoints may return:

```json
{
  "needs_confirmation": true,
  "reason": "...",
  "message": "..."
}
```

Call the same endpoint again with `force: true` to continue.

Current reasons:

- `photos_rebuild_clears_thumbnails`: photos rebuild clears cached thumbnails and rebuilds ids.
- `knowledge_base_unchanged`: selected taxa workbook path, size, and modified time match stored metadata.
- `mapping_inputs_unchanged`: photos and taxa sync times match the last mapping sync.
- `taxa_newer_than_photos`: taxa were synced later than photos.

## Operation Responses

Long-running update/rebuild endpoints return:

```json
{
  "operation": {
    "module": "photos",
    "task_id": "...",
    "running": true,
    "processed": 12,
    "total": 102
  }
}
```

Poll `GET /operations/status` for progress and final result.

## Photos

- `GET /photos/roots`: roots and `photos_metadata`.
- `PUT /photos/roots`: replace roots. Body: `{"roots": ["..."]}`.
- `GET /photos/browse?root=...&relative_dir=...`: direct child folders and files.
- `GET /photos/all`: all photo rows.
- `GET /photos/changed`: photos with status `new` or `updated`.
- `GET /photos/latest-update`: `photos_metadata`.
- `GET /photos/file/{photo_id}?v=...`: original photo. Versioned requests are long-cacheable.
- `GET /photos/thumbnail/{photo_id}?v=...`: lazy thumbnail. Versioned requests are long-cacheable.
- `GET /photos/{photo_id}`: one photo row.
- `POST /photos/update`: update one or more roots. Body: `{"root": "..."}` or `{"roots": ["..."]}`.
- `POST /photos/rebuild`: rebuild roots. Body: `{"roots": ["..."], "force": true}`.
- `GET /photos/export?table_name=...`: download `photos`, `photos_dir`, or `photos_metadata`.
- `POST /photos/export`: export one photos table to a backend path.

## Taxa

- `GET /taxa/latest-update`: `taxa_metadata` plus count.
- `PUT /taxa/knowledge-base`: save workbook path without importing. Body: `{"knowledge_base_path": "..."}`.
- `GET /taxa/by-id/{taxon_id}`: one taxon row.
- `GET /taxa/by-binomial?binomial_name=...`: one taxon row by scientific name.
- `POST /taxa/update`: update taxa. Body: `{"knowledge_base_path": "...", "force": false}`.
- `POST /taxa/rebuild`: rebuild taxa. Body: `{"knowledge_base_path": "...", "force": false}`.
- `GET /taxa/export?table_name=...`: download `taxa` or `taxa_metadata`.
- `POST /taxa/export`: export one taxa table to a backend path.

## Photos-Taxa Mapping

- `GET /mapping/photos-taxa/latest-update`: metadata plus counts.
- `GET /mapping/photos-taxa/root`: root taxonomy nodes.
- `GET /mapping/photos-taxa/taxon/{taxon_id}`: exact photo ids and child taxa.
- `GET /mapping/photos-taxa/search?name=...`: find cached taxon by Chinese/common name.
- `GET /mapping/photos-taxa/search-binomial?binomial_name=...`: find cached taxon by scientific name.
- `GET /mapping/photos-taxa/suggest?query=...&mode=name|binomial`: autocomplete cached taxa.
- `POST /mapping/photos-taxa/update`: update changed/new photos.
- `POST /mapping/photos-taxa/rebuild`: rebuild mapping from all photos.
- `GET /mapping/photos-taxa/export?table_name=...`: download a mapping table.
- `POST /mapping/photos-taxa/export`: export a mapping table to a backend path.

## Map

- `GET /map/photos`: GPS-enabled, non-deleted photo rows for map display.
- `GET /map/photos?bbox=minLng,minLat,maxLng,maxLat`: same rows filtered to a viewport.

## Local Paths

- `GET /local/select-directory`: open a native directory picker when available.
- `GET /local/select-file`: open a native file picker when available.

These routes are for local desktop use.
