# Frontend

React + Vite UI for the FastAPI backend.

## Run

From the repository root, start backend and frontend together:

```bash
python main.py
```

Or start them separately. Backend:

```bash
.venv/bin/uvicorn app.api.main:app --reload --host 127.0.0.1 --port 8000
```

Frontend:

```bash
cd frontend
npm install
npm run dev
```

Open `http://127.0.0.1:5173`. Vite proxies `/api` to `http://127.0.0.1:8000`.

## Screens

- Photos: browse by root and relative folder, view thumbnails, original images, and full photo metadata.
- Taxonomy: browse mapped taxa as a tree; search or autocomplete by Chinese name/binomial name.
- Map: placeholder page.
- Admin: manage roots and knowledge-base path; run update/rebuild operations; export CSV.

## UI Notes

- Photos and taxonomy remember selected location, scroll position, selected photo, and split-pane width in local storage.
- Image URLs include a version derived from path, modified time, and size so browser caching is safe across rebuilds.
- Original image viewer supports scrollbars, wheel zoom, double-click zoom, and drag-to-pan when zoomed.
- Backend confirmation responses are shown to the user and retried with `force: true` only after confirmation.

## Check

```bash
npm run build
```
