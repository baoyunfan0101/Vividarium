# PhytoIndex

PhytoIndex is a local plant photo index. It scans photo roots, imports a plant taxonomy workbook, maps photos to taxa, and provides a FastAPI backend with a React frontend.

## Project Layout

```text
app/
  api/                  FastAPI routes
  photos/               photo roots, metadata, directory index, thumbnails
  taxa/                 workbook import and adjacency-list taxonomy
  photos_taxa_mapping/  photo-to-taxon mapping and cached taxonomy subtree
  map/                  GPS photo selection for the map view
  operations/           local operation locks and progress
frontend/               React + Vite UI
data/                   local SQLite DB and thumbnail cache, ignored by git
main.py                 local backend/frontend launcher
scripts/                build helpers
packaging/              PyInstaller configuration
```

## Setup

```bash
python3 -m venv .venv
.venv/bin/python -m pip install -r requirements.txt

cd frontend
npm install
```

## Run

Start backend and frontend together:

```bash
python main.py
```

Open `http://127.0.0.1:5173`. API docs are at `http://127.0.0.1:8000/docs`.

Backend or frontend only:

```bash
python main.py --backend-only
python main.py --frontend-only
```

When Node or native library paths are not already available, pass them to the launcher:

```bash
python main.py --frontend-path "/path/to/node/bin" --backend-path "/path/to/native/libs"
```

Equivalent environment variables:

```bash
PHYTOINDEX_FRONTEND_PATH="/path/to/node/bin" PHYTOINDEX_BACKEND_PATH="/path/to/native/libs" python main.py
```

On Windows, environment variables use semicolon-separated paths.

## Windows Release Build

Build on Windows:

```bat
scripts\build_windows.bat
```

The script installs Python build dependencies, builds `frontend/dist`, and creates:

```text
dist\PhytoIndex.exe
```

The exe starts FastAPI, serves the built frontend, opens the browser, and stores runtime data in:

```text
%APPDATA%\PhytoIndex
```

No Python or Node installation is required on the target user's machine.

## Core Tables

| Table | Owner | Purpose |
| --- | --- | --- |
| `photos_metadata` | photos | configured roots, order, and per-root sync time |
| `photos` | photos | photo metadata, parsed scientific name, EXIF/GPS, thumbnail path, status |
| `photos_dir` | photos | derived directory index for fast folder browsing |
| `taxa_metadata` | taxa | workbook path, workbook file state, taxa sync time |
| `taxa` | taxa | taxonomy adjacency list |
| `photos_taxa_mapping_metadata` | mapping | mapping sync time and source module sync times |
| `photos_taxa_mapping` | mapping | `photo_id -> taxon_id` |
| `photos_taxa_mapping_taxa` | mapping | photographed taxonomy subtree with original taxon ids |

The map module currently reads GPS-enabled rows from `photos`; it does not own a table.

## Main Workflows

- Configure photo roots and a taxonomy workbook in Admin.
- Update or rebuild photos, taxa, and photo-taxa mapping from Admin.
- Browse photos by root/folder.
- Browse photos by mapped taxonomy tree.
- Search mapped taxa by Chinese name or binomial name.
- Browse GPS-enabled photos on a MapLibre/OpenStreetMap map.
- Export module tables as CSV.

## Checks

```bash
python -m py_compile main.py app/api/*.py app/photos/*.py app/taxa/*.py app/photos_taxa_mapping/*.py app/map/*.py

cd frontend
npm run build
```

## Module Docs

- [API](app/api/README.md)
- [Photos](app/photos/README.md)
- [Taxa](app/taxa/README.md)
- [Photos-Taxa Mapping](app/photos_taxa_mapping/README.md)
- [Map](app/map/README.md)
- [Frontend](frontend/README.md)
