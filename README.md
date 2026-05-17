# PhytoIndex

PhytoIndex is a local plant photo indexing app. It scans photo folders, extracts filename and EXIF metadata, imports a taxonomy knowledge base from Excel, maps photos to taxa, and provides a FastAPI + React interface for browsing photos by filesystem folders or taxonomy tree.

## Status

This repository is an early working version. The core local database, photo scanner, taxonomy importer, photo-taxon mapping, API routes, and React UI are implemented. The map page is reserved but not implemented yet.

## Features

- Browse photos by configured root folders and relative paths.
- Browse photographed taxa as a folder-like taxonomy tree.
- Search taxonomy folders by Chinese/display name or binomial name.
- Extract image dimensions, file size, modified time, camera, capture time, EXIF JSON, and GPS coordinates when available.
- Import taxa from the `plants` sheet of an Excel knowledge base.
- Keep taxa as an adjacency list with stable IDs on update and fresh IDs on rebuild.
- Maintain a photo-to-taxon mapping cache for photographed taxa.
- Manage photo roots and taxonomy knowledge-base path from the Admin UI.
- Update/rebuild photos, taxa, and mapping from the Admin UI.
- Download module tables as CSV.

## Project Layout

```text
PhytoIndex/
├── app/
│   ├── api/                  # FastAPI routes
│   ├── photos/               # Photo scanning, metadata, and root management
│   ├── taxa/                 # Taxonomy Excel importer and adjacency-list storage
│   └── photos_taxa_mapping/  # Photo-to-taxon mapping cache
├── frontend/                 # React + Vite frontend
├── data/                     # Local runtime data, ignored by git
├── requirements.txt          # Python dependencies
└── README.md
```

## Local Setup

Create a Python virtual environment and install backend dependencies:

```bash
python3 -m venv .venv
.venv/bin/python -m pip install -r requirements.txt
```

Install frontend dependencies:

```bash
cd frontend
npm install
```

## Run

Start the FastAPI backend from the repository root:

```bash
.venv/bin/uvicorn app.api.main:app --reload --host 127.0.0.1 --port 8000
```

Start the React frontend in another terminal:

```bash
cd frontend
npm run dev
```

Open:

```text
http://127.0.0.1:5173
```

FastAPI docs are available at:

```text
http://127.0.0.1:8000/docs
```

## Core Modules

### Photos

The `photos` module stores configured roots in `photos_metadata` and photo rows in `photos`. It indexes files under selected roots, stores relative paths, parsed binomial names, capture metadata, dimensions, file size, filesystem modified time, GPS coordinates, serialized EXIF, thumbnail path, and status.

Status values are:

- `unchanged`
- `deleted`
- `updated`
- `new`

### Taxa

The `taxa` module imports the `plants` sheet from an Excel knowledge base and stores taxa in an adjacency-list table named `taxa`. It records the knowledge-base path, file size, file modified time, and last internal sync time in `taxa_metadata`.

Update keeps the same `taxon_id` for existing binomial names. Rebuild recreates the table and may change IDs.

### Photos-Taxa Mapping

The `photos_taxa_mapping` module maps `photo_id` to `taxon_id`, keeps a cached subtree of photographed taxa, and records mapping sync metadata. Incremental update processes photos whose status is `updated` or `new`; rebuild processes all photos.

Unmapped photos are assigned a special internal taxon id and are reported by the mapping operation.

## API Notes

API route details are documented in:

- `app/api/README.md`
- `app/photos/README.md`
- `app/taxa/README.md`
- `app/photos_taxa_mapping/README.md`

Some sync endpoints may return `needs_confirmation: true`. The frontend asks the user and retries with `force: true` when confirmed.

## Development Checks

Backend compile check:

```bash
PYTHONPYCACHEPREFIX=/private/tmp/phytoindex_pycache .venv/bin/python -m py_compile app/api/*.py app/photos/*.py app/taxa/*.py app/photos_taxa_mapping/*.py
```

Frontend build check:

```bash
cd frontend
npm run build
```

