# Map

The map module exposes photos that can be displayed geographically. It is read-only and depends only on `photos`.

## Scope

- Include non-deleted photos with both `longitude` and `latitude`.
- Do not depend on taxonomy or photo-taxa mapping.
- Keep base-map configuration outside this module; the frontend reads the selected tile provider from local storage.

## Public API

Import from `app.map`.

- `list_map_photos(db_path=DEFAULT_DB_PATH, bbox=None)`: return GPS-enabled photo rows. `bbox` is `(min_lng, min_lat, max_lng, max_lat)`.

Returned fields match the photo fields needed by the frontend preview and cache-versioned image URLs:

| Field |
| --- |
| `photo_id` |
| `root` |
| `relative_path` |
| `parent_dir` |
| `path_depth` |
| `filename` |
| `binomial_name` |
| `captured_at` |
| `location` |
| `camera` |
| `width` |
| `height` |
| `file_size` |
| `modified_at` |
| `longitude` |
| `latitude` |
| `exif_json` |
| `thumbnail_path` |
| `status` |

## Backend Routes

- `GET /map/photos`: all GPS-enabled photo rows.
- `GET /map/photos?bbox=minLng,minLat,maxLng,maxLat`: rows inside a viewport.

## Frontend Design

- Renderer: MapLibre GL JS.
- Default tile source: OpenStreetMap raster tiles, configured from Admin.
- Clustering: MapLibre client-side GeoJSON clustering.
- State cache: center, zoom, selected photo, preview photo, and image/details mode are stored in local storage.
- Preview: selected map photos use the same image/details component as Photos and Taxonomy.

## Extension Points

- Add more tile providers in `frontend/src/features/map/mapProviders.ts`.
- Add server-side clustering only if client-side clustering becomes too slow for the photo count.
- Support local/offline tiles by adding a provider that points to a local tile server.
