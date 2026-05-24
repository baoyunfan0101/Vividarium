# PhytoIndex

PhytoIndex is a local plant photo index. It scans photo roots, imports a plant taxonomy workbook, maps photos to taxa, and provides a FastAPI backend with a React frontend for browsing by filesystem path or taxonomy tree.

## Status

Working local version. The map page exists in the UI but is not implemented.

## Layout

```text
app/
  api/                  FastAPI routes
  photos/               photo roots, scanning, metadata, thumbnails
  taxa/                 Excel taxonomy import and adjacency-list storage
  photos_taxa_mapping/  photo-to-taxon mapping cache
  operations/           in-process operation state/progress
frontend/               React + Vite UI
data/                   local SQLite DB and thumbnail cache, ignored by git
```

## Setup

```bash
python3 -m venv .venv
.venv/bin/python -m pip install -r requirements.txt

cd frontend
npm install
```

## Run

Start both backend and frontend:

```bash
python main.py
```

Open `http://127.0.0.1:5173`. API docs are at `http://127.0.0.1:8000/docs`.

Optional single-process commands:

```bash
.venv/bin/uvicorn app.api.main:app --reload --host 127.0.0.1 --port 8000

cd frontend
npm run dev
```

## Main Features

- Configure and manage multiple photo roots.
- Browse photos by root and relative folder.
- Browse mapped taxa as a folder-like tree.
- Search mapped taxa by Chinese name or binomial name, with suggestions from the mapping subtree.
- Import taxonomy from the `plants` sheet of an Excel workbook.
- Update or rebuild photos, taxa, and photo-taxa mapping from the Admin UI.
- Export module tables as CSV.
- Generate thumbnails lazily on first request.

## Data Model

- `photos_metadata`: configured roots, sort order, and per-root sync time.
- `photos`: photo metadata, parsed binomial name, EXIF/GPS, thumbnail path, and status.
- `photos_dir`: derived directory index for fast folder browsing.
- `taxa_metadata`: knowledge-base path, file size, modified time, and sync time.
- `taxa`: taxonomy adjacency list.
- `photos_taxa_mapping_metadata`: mapping sync time and source module sync times.
- `photos_taxa_mapping`: `photo_id -> taxon_id`.
- `photos_taxa_mapping_taxa`: cached taxonomy subtree used by photographed taxa.

## Module Docs

- [API](app/api/README.md)
- [Photos](app/photos/README.md)
- [Taxa](app/taxa/README.md)
- [Photos-Taxa Mapping](app/photos_taxa_mapping/README.md)
- [Frontend](frontend/README.md)

## Checks

```bash
python -m py_compile app/api/*.py app/photos/*.py app/taxa/*.py app/photos_taxa_mapping/*.py

cd frontend
npm run build
```
