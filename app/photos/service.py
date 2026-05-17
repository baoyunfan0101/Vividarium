"""Public service API for rebuilding, updating, and querying photos."""

from __future__ import annotations

import csv
from pathlib import Path

from .db import (
    DEFAULT_DB_PATH,
    STATUS_DELETED,
    STATUS_NEW,
    STATUS_UNCHANGED,
    STATUS_UPDATED,
    PhotoRecord,
    PhotosDatabase,
)
from .filename_parser import parse_photo_filename
from .scanner import iter_image_files, read_file_metadata


def rebuild_photos(
    roots: list[str | Path],
    db_path: str | Path = DEFAULT_DB_PATH,
) -> dict[str, int]:
    """Rebuild photo rows for the given roots."""

    normalized_roots = [_normalize_root(root) for root in roots]
    with PhotosDatabase(db_path) as db:
        db.clear_photos_for_roots(normalized_roots)
        inserted = 0
        for root in normalized_roots:
            for path in iter_image_files(root):
                db.insert_photo(_build_record(root, path, STATUS_NEW))
                inserted += 1
            db.upsert_root(root)

    return {"roots": len(normalized_roots), "inserted": inserted}


def update_photos(
    root: str | Path,
    db_path: str | Path = DEFAULT_DB_PATH,
) -> dict[str, int]:
    """Update one root using file size and modified time checks."""

    normalized_root = _normalize_root(root)
    scanned_paths: set[str] = set()
    unchanged = 0
    updated = 0
    inserted = 0

    with PhotosDatabase(db_path) as db:
        for path in iter_image_files(normalized_root):
            relative_path = path.relative_to(normalized_root).as_posix()
            scanned_paths.add(relative_path)
            stat = path.stat()
            existing = db.get_photo_by_path(normalized_root, relative_path)

            if existing is None:
                db.insert_photo(_build_record(normalized_root, path, STATUS_NEW))
                inserted += 1
                continue

            if (
                existing["file_size"] == stat.st_size
                and existing["modified_at"] == stat.st_mtime
            ):
                db.set_status(existing["photo_id"], STATUS_UNCHANGED)
                unchanged += 1
                continue

            db.update_photo(
                existing["photo_id"],
                _build_record(normalized_root, path, STATUS_UPDATED),
            )
            updated += 1

        deleted = 0
        for photo in db.list_photos_for_root(normalized_root):
            if photo["relative_path"] not in scanned_paths and photo["status"] != STATUS_DELETED:
                db.set_status(photo["photo_id"], STATUS_DELETED)
                deleted += 1
        db.upsert_root(normalized_root)

    return {
        "unchanged": unchanged,
        "updated": updated,
        "new": inserted,
        "deleted": deleted,
    }


def list_directory(
    root: str | Path,
    relative_dir: str | Path = "",
    db_path: str | Path = DEFAULT_DB_PATH,
) -> dict:
    """Return direct child directories and files under root/relative_dir."""

    with PhotosDatabase(db_path) as db:
        return db.list_directory(_normalize_root(root), str(relative_dir))


def get_photo(
    photo_id: int,
    db_path: str | Path = DEFAULT_DB_PATH,
) -> dict | None:
    with PhotosDatabase(db_path) as db:
        return db.get_photo_by_id(photo_id)


def list_photos(db_path: str | Path = DEFAULT_DB_PATH) -> list[dict]:
    with PhotosDatabase(db_path) as db:
        return db.list_photos()


def list_changed_photos(db_path: str | Path = DEFAULT_DB_PATH) -> list[dict]:
    with PhotosDatabase(db_path) as db:
        return db.list_changed_photos()


def get_roots(db_path: str | Path = DEFAULT_DB_PATH) -> list[str]:
    with PhotosDatabase(db_path) as db:
        return db.list_roots()


def save_roots(
    roots: list[str | Path],
    db_path: str | Path = DEFAULT_DB_PATH,
) -> list[dict]:
    normalized_roots = [_normalize_root(root) for root in roots]
    with PhotosDatabase(db_path) as db:
        db.replace_roots(normalized_roots)
        return db.list_metadata()


def get_latest_update(db_path: str | Path = DEFAULT_DB_PATH) -> list[dict]:
    with PhotosDatabase(db_path) as db:
        return db.list_metadata()


def export_table_csv(
    table_name: str,
    output_path: str | Path,
    db_path: str | Path = DEFAULT_DB_PATH,
) -> int:
    """Export one photos module table to CSV."""

    if table_name not in {"photos", "photos_metadata"}:
        raise ValueError("table_name must be 'photos' or 'photos_metadata'")

    output = Path(output_path)
    output.parent.mkdir(parents=True, exist_ok=True)

    with PhotosDatabase(db_path) as db:
        fieldnames, rows = db.export_rows(table_name)

    with output.open("w", newline="", encoding="utf-8-sig") as file:
        writer = csv.DictWriter(file, fieldnames=fieldnames)
        writer.writeheader()
        writer.writerows(rows)

    return len(rows)


def _build_record(root: str, path: Path, status: str) -> PhotoRecord:
    root_path = Path(root)
    parsed = parse_photo_filename(path.name)
    metadata = read_file_metadata(path)
    stat = path.stat()
    return PhotoRecord(
        root=root,
        relative_path=path.relative_to(root_path).as_posix(),
        filename=path.name,
        binomial_name=parsed.binomial_name,
        captured_at=metadata.captured_at or parsed.shoot_date,
        location=parsed.location,
        camera=metadata.camera or parsed.device,
        width=metadata.width,
        height=metadata.height,
        file_size=stat.st_size,
        modified_at=stat.st_mtime,
        longitude=metadata.longitude,
        latitude=metadata.latitude,
        exif_json=metadata.exif_json,
        thumbnail_path=None,
        status=status,
    )


def _normalize_root(root: str | Path) -> str:
    return str(Path(root).expanduser().resolve())
