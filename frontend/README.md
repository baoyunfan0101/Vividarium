# PhytoIndex Frontend

React + Vite frontend for the FastAPI backend.

## Run

Start the backend first:

```bash
.venv/bin/uvicorn app.api.main:app --reload --host 127.0.0.1 --port 8000
```

Install frontend dependencies and run Vite:

```bash
cd frontend
npm install
npm run dev
```

Open:

```text
http://127.0.0.1:5173
```

The Vite dev server proxies `/api` to `http://127.0.0.1:8000`.

## Current Features

- Browse photos by recorded root and relative folder path.
- Preview original photo files through the backend photo file endpoint.
- Browse mapped taxa as a folder-like taxonomy tree.
- Search mapped taxa by display name or binomial name.
- View photos with `updated` or `new` status.
- Trigger photos, taxa, and photos-taxa mapping update/rebuild operations.
- Download photos, taxa, and photos-taxa mapping tables as CSV files.
- Handle backend confirmation responses by asking the user and retrying with `force: true`.
