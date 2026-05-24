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
| Taxonomy | browse mapped taxa; search/autocomplete by Chinese or binomial name |
| Map | browse GPS photos with MapLibre clustering and a photo preview overlay |
| Admin | manage roots/workbook path, run operations, export CSV, choose map tile provider |

## Local UI State

Stored in `localStorage`:

- active top-level screen
- Photos and Taxonomy location, selection, scroll position, and split-pane width
- Taxonomy search mode and query
- Map center, zoom, selected photo, open preview, preview mode, and tile provider

## Image Loading

- Original and thumbnail URLs include a version derived from path, modified time, and file size.
- Versioned image responses are safe for browser caching.
- Thumbnails are generated lazily by the backend on first request.
- The original image viewer supports scrollbars, wheel zoom, double-click zoom, and drag-to-pan when zoomed.

## Check

```bash
npm run build
```
