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
  src/
    components/         shared navigation, status, virtual list, photo viewer
    features/           Admin, Photos, Taxonomy, and Map screens
    lib/                browser, path, photo, storage, taxon, split-pane helpers
    styles/             global, layout, component, and feature CSS
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

Use `main.py` to start the app:

```bash
python main.py
```

Options:

- `--backend-only`: start only FastAPI.
- `--frontend-only`: start only Vite.
- `--frontend-path PATH`: prepend Node/npm paths.
- `--backend-path PATH`: prepend native library paths used by Python packages.

Example:

```bash
python main.py --frontend-path "/path/to/node/bin" --backend-path "/path/to/native/libs"
```

Equivalent environment variables are `PHYTOINDEX_FRONTEND_PATH` and `PHYTOINDEX_BACKEND_PATH`. On Windows, use semicolon-separated paths.

Manual development run:

```bash
.venv/bin/uvicorn app.api.main:app --reload --host 127.0.0.1 --port 8000

cd frontend
npm run dev
```

Open `http://127.0.0.1:5173`. API docs are at `http://127.0.0.1:8000/docs`.

## Release

Windows packaging is handled by PyInstaller:

```bat
scripts\build_windows.bat
```

The script builds the frontend and creates `dist\PhytoIndex.exe`. The packaged app starts FastAPI, serves the built frontend, opens the browser, and stores runtime data under `%APPDATA%\PhytoIndex`.

Target users do not need Python or Node installed.

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
- Load large photo folders through cursor paging backed by `photos_dir` and photos browse indexes.
- Browse photos by mapped taxonomy tree.
- Search mapped taxa by Chinese name or binomial name.
- Browse GPS-enabled photos on a MapLibre/OpenStreetMap map.
- Export module tables as CSV.

## Module Docs

- [API](app/api/README.md)
- [Photos](app/photos/README.md)
- [Taxa](app/taxa/README.md)
- [Photos-Taxa Mapping](app/photos_taxa_mapping/README.md)
- [Map](app/map/README.md)
- [Frontend](frontend/README.md)
