"""Public service API for rebuilding, updating, and querying photos."""

from __future__ import annotations

import base64
import csv
import json
import shutil
from pathlib import Path
from typing import Callable

from app.runtime_paths import thumbnail_dir

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
    thumbnail_root: str | Path = thumbnail_dir(),
    progress: Callable[[int, int | None, str | None], None] | None = None,
) -> dict[str, int]:
    """Rebuild photo rows for the given roots."""

    normalized_roots = [_normalize_root(root) for root in roots]
    root_files = [(root, list(iter_image_files(root))) for root in normalized_roots]
    total = sum(len(paths) for _, paths in root_files)
    _report_progress(progress, 0, total, "Scanning photos")
    thumbnails_cleared = _clear_thumbnail_cache(thumbnail_root)
    with PhotosDatabase(db_path) as db:
        db.clear_photos()
        inserted = 0
        for root, paths in root_files:
            for path in paths:
                db.insert_photo(_build_record(root, path, STATUS_NEW))
                inserted += 1
                _report_progress(progress, inserted, total, f"Rebuilding {root}")
            db.refresh_directories_for_roots([root])
            db.upsert_root(root)

    return {
        "roots": len(normalized_roots),
        "inserted": inserted,
        "thumbnails_cleared": thumbnails_cleared,
    }


def update_photos(
    root: str | Path,
    db_path: str | Path = DEFAULT_DB_PATH,
    progress: Callable[[int, int | None, str | None], None] | None = None,
) -> dict[str, int]:
    """Update one root using file size and modified time checks."""

    normalized_root = _normalize_root(root)
    scanned_paths: set[str] = set()
    unchanged = 0
    updated = 0
    inserted = 0
    image_files = list(iter_image_files(normalized_root))
    total = len(image_files)
    processed = 0
    _report_progress(progress, 0, total, f"Updating {normalized_root}")

    with PhotosDatabase(db_path) as db:
        other_roots_unchanged = db.set_other_roots_unchanged(normalized_root)
        for path in image_files:
            relative_path = path.relative_to(normalized_root).as_posix()
            scanned_paths.add(relative_path)
            stat = path.stat()
            existing = db.get_photo_by_path(normalized_root, relative_path)
            processed += 1

            if existing is None:
                db.insert_photo(_build_record(normalized_root, path, STATUS_NEW))
                inserted += 1
                _report_progress(progress, processed, total, f"Updating {normalized_root}")
                continue

            if (
                existing["file_size"] == stat.st_size
                and existing["modified_at"] == stat.st_mtime
            ):
                db.set_status(existing["photo_id"], STATUS_UNCHANGED)
                unchanged += 1
                _report_progress(progress, processed, total, f"Updating {normalized_root}")
                continue

            db.update_photo(
                existing["photo_id"],
                _build_record(normalized_root, path, STATUS_UPDATED),
            )
            updated += 1
            _report_progress(progress, processed, total, f"Updating {normalized_root}")

        deleted = 0
        for photo in db.list_photos_for_root(normalized_root):
            if photo["relative_path"] not in scanned_paths and photo["status"] != STATUS_DELETED:
                db.set_status(photo["photo_id"], STATUS_DELETED)
                deleted += 1
        db.refresh_directories_for_roots([normalized_root])
        db.upsert_root(normalized_root)

    return {
        "unchanged": unchanged,
        "updated": updated,
        "new": inserted,
        "deleted": deleted,
        "other_roots_unchanged": other_roots_unchanged,
    }


def _report_progress(
    progress: Callable[[int, int | None, str | None], None] | None,
    processed: int,
    total: int | None,
    message: str | None,
) -> None:
    if progress is not None:
        progress(processed, total, message)


def list_directory(
    root: str | Path,
    relative_dir: str | Path = "",
    db_path: str | Path = DEFAULT_DB_PATH,
) -> dict:
    """Return direct child directories and files under root/relative_dir."""

    with PhotosDatabase(db_path) as db:
        return db.list_directory(_normalize_root(root), str(relative_dir))


def list_directory_page(
    root: str | Path,
    relative_dir: str | Path = "",
    cursor: str | None = None,
    limit: int = 100,
    db_path: str | Path = DEFAULT_DB_PATH,
) -> dict:
    """Return one keyset-cursor page of child directories and files."""

    normalized_root = _normalize_root(root)
    directory = str(relative_dir)
    page_limit = max(1, min(limit, 500))
    cursor_data = _decode_directory_cursor(cursor)
    section = cursor_data.get("section", "directories")
    remaining = page_limit
    directories: list[str] = []
    files: list[dict] = []

    with PhotosDatabase(db_path) as db:
        counts = db.count_directory_items(normalized_root, directory)

        if section == "directories":
            directories = db.list_directory_dirs_page(
                normalized_root,
                directory,
                after_name=cursor_data.get("name"),
                limit=remaining,
            )
            remaining -= len(directories)
            if remaining == 0:
                next_cursor = _encode_directory_cursor({
                    "section": "directories",
                    "name": directories[-1],
                })
                return _directory_page_result(normalized_root, directory, directories, files, next_cursor, counts)
            section = "files"
            cursor_data = {}

        if section == "files" and remaining > 0:
            files = db.list_directory_files_page(
                normalized_root,
                directory,
                after_filename=cursor_data.get("filename"),
                after_photo_id=cursor_data.get("photo_id"),
                limit=remaining,
            )

        next_cursor = None
        if files:
            last_file = files[-1]
            if _has_next_file_page(db, normalized_root, directory, last_file):
                next_cursor = _encode_directory_cursor({
                    "section": "files",
                    "filename": last_file["filename"],
                    "photo_id": last_file["photo_id"],
                })

    return _directory_page_result(normalized_root, directory, directories, files, next_cursor, counts)


def get_photo(
    photo_id: int,
    db_path: str | Path = DEFAULT_DB_PATH,
) -> dict | None:
    with PhotosDatabase(db_path) as db:
        return db.get_photo_by_id(photo_id)


def get_or_create_thumbnail(
    photo_id: int,
    db_path: str | Path = DEFAULT_DB_PATH,
    thumbnail_root: str | Path = thumbnail_dir(),
    size: tuple[int, int] = (256, 256),
) -> Path | None:
    with PhotosDatabase(db_path) as db:
        photo = db.get_photo_by_id(photo_id)
        if photo is None:
            return None

        existing = photo.get("thumbnail_path")
        if existing:
            existing_path = Path(existing)
            if existing_path.exists() and existing_path.is_file():
                return existing_path

        source_path = _photo_file_path(photo)
        if not source_path.exists() or not source_path.is_file():
            raise FileNotFoundError(source_path)

        output_path = _thumbnail_path(photo, thumbnail_root)
        _create_thumbnail(source_path, output_path, size)
        db.set_thumbnail_path(photo_id, output_path.as_posix())
        return output_path


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

    if table_name not in {"photos", "photos_metadata", "photos_dir"}:
        raise ValueError(
            "table_name must be 'photos', 'photos_metadata', or 'photos_dir'"
        )

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
        parent_dir=_parent_dir(path.relative_to(root_path).as_posix()),
        path_depth=_path_depth(_parent_dir(path.relative_to(root_path).as_posix())),
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


def _photo_file_path(photo: dict) -> Path:
    root = Path(photo["root"]).expanduser().resolve()
    candidate = (root / photo["relative_path"]).resolve()
    candidate.relative_to(root)
    return candidate


def _thumbnail_path(photo: dict, thumbnail_root: str | Path) -> Path:
    modified = int(photo.get("modified_at") or 0)
    file_size = int(photo.get("file_size") or 0)
    filename = f"photo_{photo['photo_id']}_{modified}_{file_size}.webp"
    return Path(thumbnail_root) / filename


def _clear_thumbnail_cache(thumbnail_root: str | Path) -> int:
    root = Path(thumbnail_root)
    if not root.exists() or not root.is_dir():
        return 0
    count = sum(1 for path in root.rglob("*") if path.is_file())
    shutil.rmtree(root)
    return count


def _create_thumbnail(
    source_path: Path,
    output_path: Path,
    size: tuple[int, int],
) -> None:
    try:
        from PIL import Image, ImageOps
    except ImportError as exc:
        raise RuntimeError("Pillow is required to generate thumbnails") from exc

    output_path.parent.mkdir(parents=True, exist_ok=True)
    with Image.open(source_path) as image:
        image = ImageOps.exif_transpose(image)
        image.thumbnail(size)
        if image.mode not in {"RGB", "L"}:
            image = image.convert("RGB")
        image.save(output_path, "WEBP", quality=58, method=6)


def _parent_dir(relative_path: str) -> str:
    parent = Path(relative_path).parent.as_posix()
    return "" if parent == "." else parent


def _path_depth(relative_dir: str) -> int:
    return 0 if not relative_dir else len(relative_dir.split("/"))


def _directory_page_result(
    root: str,
    relative_dir: str | Path,
    directories: list[str],
    files: list[dict],
    next_cursor: str | None,
    counts: dict,
) -> dict:
    return {
        "root": root,
        "relative_dir": str(relative_dir),
        "directories": directories,
        "files": files,
        "next_cursor": next_cursor,
        "directory_count": counts["directory_count"],
        "file_count": counts["file_count"],
    }


def _encode_directory_cursor(value: dict) -> str:
    payload = json.dumps(value, ensure_ascii=True, separators=(",", ":")).encode("utf-8")
    return base64.urlsafe_b64encode(payload).decode("ascii").rstrip("=")


def _decode_directory_cursor(cursor: str | None) -> dict:
    if not cursor:
        return {}
    padding = "=" * (-len(cursor) % 4)
    try:
        value = json.loads(base64.urlsafe_b64decode((cursor + padding).encode("ascii")))
    except (ValueError, json.JSONDecodeError):
        return {}
    return value if isinstance(value, dict) else {}


def _has_next_file_page(
    db: PhotosDatabase,
    root: str,
    relative_dir: str | Path,
    last_file: dict,
) -> bool:
    return bool(db.list_directory_files_page(
        root,
        str(relative_dir),
        after_filename=last_file["filename"],
        after_photo_id=int(last_file["photo_id"]),
        limit=1,
    ))
