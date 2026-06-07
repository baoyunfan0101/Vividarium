# Frontend

React + Vite UI for the FastAPI backend.

## Run

From the repository root:

```bash
python main.py
```

If `npm` is not on `PATH`:

```bash
python main.py --frontend-path "/path/to/node/bin"
```

Frontend only:

```bash
cd frontend
npm install
npm run dev
```

Open `http://127.0.0.1:5173`. Vite proxies `/api` to `http://127.0.0.1:8000`.

## Screens

| Screen | Purpose |
| --- | --- |
| Photos | browse by root/folder; open thumbnails, originals, and metadata |
| Taxonomy | browse mapped taxa; search/autocomplete by Chinese or scientific name |
| Map | browse GPS photos with MapLibre clustering and a photo preview overlay |
| Admin | manage roots/workbook path, run operations, export CSV, choose map tile provider |

## Source Layout

```text
src/
  App.tsx                  top-level navigation and screen routing
  api.ts                   typed HTTP client for backend routes
  components/
    NavButton.tsx          top navigation button
    photo.tsx              thumbnails, grid, image viewer, metadata panel
    status.tsx             shared loading and operation status display
    virtual.tsx            virtualized file/taxon list
  features/
    admin/                 roots, workbook, mapping, export, and operation controls
    photos/                root/folder browser backed by cursor paging
    taxonomy/              mapped taxonomy browser and autocomplete search
    map/                   MapLibre map, providers, layers, preview panel, map state
  lib/
    browserUtils.ts        type select and viewport/list helpers
    pathUtils.ts           path joining and relative path helpers
    photoUtils.ts          photo URL versioning and photo metadata helpers
    storage.ts             localStorage wrappers
    taxonUtils.ts          taxon display-name helpers
    useResizableSplit.ts   persisted split-pane sizing
  styles/
    base/                  reset and scrollbar rules
    layout/                app shell and responsive rules
    components/            browser, toolbar, panel, photo, loading styles
    features/              admin and map styles
```

## Local UI State

Stored in `localStorage`:

- active top-level screen
- Photos and Taxonomy location, selection, scroll position, and split-pane width
- Taxonomy search query
- Map center, zoom, selected photo, open preview, preview mode, and tile provider

## Browsing

- Photos uses `/api/photos/browse-page` with a keyset cursor; the UI appends pages when the file list or thumbnail grid nears the end.
- Taxonomy uses the cached photos-taxa mapping tree and photo ids returned by mapping endpoints.
- Both browsers use a virtualized left list, a thumbnail grid, Finder-like type select, and persisted split-pane width.
- Directory and taxon summary counts are shown below the left browser pane.

## Image Loading

- Original and thumbnail URLs include a version derived from path, modified time, and file size.
- Versioned image responses are safe for browser caching.
- Thumbnails are generated lazily by the backend on first request.
- The original image viewer supports centered fit-to-view, wheel zoom, double-click zoom, and drag-to-pan.
