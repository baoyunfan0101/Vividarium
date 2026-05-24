"""Read-only helpers for photos that can be shown on a map."""

from __future__ import annotations

from pathlib import Path

from app.photos import list_photos
from app.photos.db import DEFAULT_DB_PATH, STATUS_DELETED


MAP_PHOTO_FIELDS = [
    "photo_id",
    "root",
    "relative_path",
    "parent_dir",
    "path_depth",
    "filename",
    "binomial_name",
    "captured_at",
    "location",
    "camera",
    "width",
    "height",
    "file_size",
    "modified_at",
    "longitude",
    "latitude",
    "exif_json",
    "thumbnail_path",
    "status",
]


def list_map_photos(
    db_path: str | Path = DEFAULT_DB_PATH,
    bbox: tuple[float, float, float, float] | None = None,
) -> list[dict]:
    """Return non-deleted photos that have both longitude and latitude."""

    rows = []
    for photo in list_photos(db_path=db_path):
        if photo.get("status") == STATUS_DELETED:
            continue
        longitude = photo.get("longitude")
        latitude = photo.get("latitude")
        if longitude is None or latitude is None:
            continue
        if bbox is not None and not _inside_bbox(float(longitude), float(latitude), bbox):
            continue
        rows.append({field: photo.get(field) for field in MAP_PHOTO_FIELDS})
    return rows


def _inside_bbox(
    longitude: float,
    latitude: float,
    bbox: tuple[float, float, float, float],
) -> bool:
    min_lng, min_lat, max_lng, max_lat = bbox
    return min_lng <= longitude <= max_lng and min_lat <= latitude <= max_lat
