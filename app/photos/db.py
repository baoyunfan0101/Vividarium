"""SQLite access layer for photo metadata."""

from __future__ import annotations

import sqlite3
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

from app.runtime_paths import database_path

DEFAULT_DB_PATH = database_path()

STATUS_UNCHANGED = "unchanged"
STATUS_DELETED = "deleted"
STATUS_UPDATED = "updated"
STATUS_NEW = "new"


@dataclass(frozen=True)
class PhotoRecord:
    root: str
    relative_path: str
    parent_dir: str
    path_depth: int
    filename: str
    binomial_name: str | None = None
    captured_at: str | None = None
    location: str | None = None
    camera: str | None = None
    width: int | None = None
    height: int | None = None
    file_size: int | None = None
    modified_at: float | None = None
    longitude: float | None = None
    latitude: float | None = None
    exif_json: str | None = None
    thumbnail_path: str | None = None
    status: str = STATUS_NEW


class PhotosDatabase:
    """Photo table operations and query interface."""

    def __init__(self, db_path: str | Path = DEFAULT_DB_PATH) -> None:
        self.db_path = Path(db_path)
        self.db_path.parent.mkdir(parents=True, exist_ok=True)
        self._conn = sqlite3.connect(self.db_path)
        self._conn.row_factory = sqlite3.Row
        self.init_schema()

    def __enter__(self) -> "PhotosDatabase":
        return self

    def __exit__(self, *_exc: object) -> None:
        self.close()

    def close(self) -> None:
        self._conn.close()

    def init_schema(self) -> None:
        self._ensure_photos_table()
        self._ensure_dir_table()
        self._ensure_metadata_table()
        self._conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_photos_root_path ON photos(root, relative_path)"
        )
        self._conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_photos_browse ON photos(root, parent_dir, status, filename)"
        )
        self._conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_photos_browse_cursor ON photos(root, parent_dir, status, filename, photo_id)"
        )
        self._conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_photos_status ON photos(status)"
        )
        self._conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_photos_binomial_name ON photos(binomial_name)"
        )
        self._conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_photos_dir_unique ON photos_dir(root, relative_dir)"
        )
        self._conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_photos_dir_browse ON photos_dir(root, parent_dir, name)"
        )
        self._conn.commit()

    def _ensure_photos_table(self) -> None:
        row = self._conn.execute(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'photos'"
        ).fetchone()
        if row is None:
            self._create_photos_table()
            self._conn.commit()
            return

        columns = [
            column["name"]
            for column in self._conn.execute("PRAGMA table_info(photos)").fetchall()
        ]
        desired = [
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
        if columns == desired:
            return

        with self._conn:
            self._conn.execute("DROP TABLE photos")
            self._create_photos_table()
            self._conn.execute("DELETE FROM sqlite_sequence WHERE name = 'photos'")

    def _ensure_metadata_table(self) -> None:
        row = self._conn.execute(
            """
            SELECT name FROM sqlite_master
            WHERE type = 'table' AND name = 'photos_metadata'
            """
        ).fetchone()
        if row is None:
            self._create_metadata_table()
            return

        rows = self._conn.execute("PRAGMA table_info(photos_metadata)").fetchall()
        columns = [column["name"] for column in rows]
        desired = ["root", "last_synced_at", "sort_order"]
        nullable_last_synced = any(
            column["name"] == "last_synced_at" and not column["notnull"]
            for column in rows
        )
        if columns == desired and nullable_last_synced:
            return

        existing_rows = self._conn.execute(
            "SELECT * FROM photos_metadata ORDER BY root"
        ).fetchall()
        with self._conn:
            self._conn.execute("DROP TABLE photos_metadata")
            self._create_metadata_table()
            self._conn.executemany(
                """
                INSERT INTO photos_metadata (root, last_synced_at, sort_order)
                VALUES (?, ?, ?)
                """,
                (
                    (
                        row["root"],
                        row["last_synced_at"] if "last_synced_at" in columns else None,
                        index,
                    )
                    for index, row in enumerate(existing_rows)
                ),
            )

    def _create_photos_table(self) -> None:
        self._conn.execute(
            """
            CREATE TABLE photos (
                photo_id INTEGER PRIMARY KEY AUTOINCREMENT,
                root TEXT NOT NULL,
                relative_path TEXT NOT NULL,
                parent_dir TEXT NOT NULL,
                path_depth INTEGER NOT NULL,
                filename TEXT NOT NULL,
                binomial_name TEXT,
                captured_at TEXT,
                location TEXT,
                camera TEXT,
                width INTEGER,
                height INTEGER,
                file_size INTEGER,
                modified_at REAL,
                longitude REAL,
                latitude REAL,
                exif_json TEXT,
                thumbnail_path TEXT DEFAULT NULL,
                status TEXT NOT NULL,
                UNIQUE(root, relative_path)
            )
            """
        )

    def _ensure_dir_table(self) -> None:
        row = self._conn.execute(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'photos_dir'"
        ).fetchone()
        if row is None:
            self._create_dir_table()
            self.refresh_directories()
            return

        columns = [
            column["name"]
            for column in self._conn.execute("PRAGMA table_info(photos_dir)").fetchall()
        ]
        desired = ["root", "relative_dir", "parent_dir", "name", "path_depth"]
        if columns == desired:
            return

        with self._conn:
            self._conn.execute("DROP TABLE photos_dir")
            self._create_dir_table()
        self.refresh_directories()

    def _create_dir_table(self) -> None:
        self._conn.execute(
            """
            CREATE TABLE photos_dir (
                root TEXT NOT NULL,
                relative_dir TEXT NOT NULL,
                parent_dir TEXT NOT NULL,
                name TEXT NOT NULL,
                path_depth INTEGER NOT NULL,
                PRIMARY KEY (root, relative_dir)
            )
            """
        )

    def _create_metadata_table(self) -> None:
        self._conn.execute(
            """
            CREATE TABLE photos_metadata (
                root TEXT PRIMARY KEY,
                last_synced_at TEXT,
                sort_order INTEGER NOT NULL
            )
            """
        )

    def set_roots(self, roots: list[str]) -> None:
        now = _now()
        with self._conn:
            self._conn.execute("DELETE FROM photos_metadata")
            self._conn.executemany(
                """
                INSERT INTO photos_metadata (root, last_synced_at, sort_order)
                VALUES (?, ?, ?)
                """,
                ((root, now, index) for index, root in enumerate(roots)),
            )

    def replace_roots(self, roots: list[str]) -> None:
        existing = {
            row["root"]: row["last_synced_at"]
            for row in self._conn.execute(
                "SELECT root, last_synced_at FROM photos_metadata"
            ).fetchall()
        }
        with self._conn:
            self._conn.execute("DELETE FROM photos_metadata")
            self._conn.executemany(
                """
                INSERT INTO photos_metadata (root, last_synced_at, sort_order)
                VALUES (?, ?, ?)
                """,
                (
                    (root, existing.get(root), index)
                    for index, root in enumerate(roots)
                ),
            )

    def upsert_root(self, root: str) -> None:
        now = _now()
        row = self._conn.execute(
            "SELECT sort_order FROM photos_metadata WHERE root = ?",
            (root,),
        ).fetchone()
        if row is None:
            sort_order = self._next_sort_order()
        else:
            sort_order = int(row["sort_order"])
        with self._conn:
            self._conn.execute(
                """
                INSERT INTO photos_metadata (root, last_synced_at, sort_order)
                VALUES (?, ?, ?)
                ON CONFLICT(root) DO UPDATE SET
                    last_synced_at = excluded.last_synced_at,
                    sort_order = excluded.sort_order
                """,
                (root, now, sort_order),
            )

    def list_roots(self) -> list[str]:
        rows = self._conn.execute(
            "SELECT root FROM photos_metadata ORDER BY sort_order, root"
        ).fetchall()
        return [row["root"] for row in rows]

    def list_metadata(self) -> list[dict]:
        rows = self._conn.execute(
            """
            SELECT
                photos_metadata.root,
                photos_metadata.last_synced_at,
                photos_metadata.sort_order,
                COALESCE(photo_counts.photo_count, 0) AS photo_count
            FROM photos_metadata
            LEFT JOIN (
                SELECT root, COUNT(*) AS photo_count
                FROM photos
                GROUP BY root
            ) AS photo_counts
                ON photo_counts.root = photos_metadata.root
            ORDER BY photos_metadata.sort_order, photos_metadata.root
            """
        ).fetchall()
        return [dict(row) for row in rows]

    def _next_sort_order(self) -> int:
        row = self._conn.execute(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 AS next_order FROM photos_metadata"
        ).fetchone()
        return int(row["next_order"])

    def export_rows(self, table_name: str) -> tuple[list[str], list[dict]]:
        if table_name not in {"photos", "photos_metadata", "photos_dir"}:
            raise ValueError(
                "table_name must be 'photos', 'photos_metadata', or 'photos_dir'"
            )
        fieldnames = [
            column["name"]
            for column in self._conn.execute(f"PRAGMA table_info({table_name})")
        ]
        rows = self._conn.execute(f"SELECT * FROM {table_name}").fetchall()
        return fieldnames, [dict(row) for row in rows]

    def clear_photos(self) -> None:
        with self._conn:
            self._conn.execute("DELETE FROM photos")
            self._conn.execute("DELETE FROM photos_dir")
            self._conn.execute("DELETE FROM sqlite_sequence WHERE name = 'photos'")

    def clear_photos_for_roots(self, roots: list[str]) -> None:
        with self._conn:
            self._conn.executemany(
                "DELETE FROM photos WHERE root = ?",
                ((root,) for root in roots),
            )
            self._conn.executemany(
                "DELETE FROM photos_dir WHERE root = ?",
                ((root,) for root in roots),
            )

    def insert_photo(self, record: PhotoRecord) -> int:
        with self._conn:
            cursor = self._conn.execute(
                """
                INSERT INTO photos (
                    root, relative_path, parent_dir, path_depth, filename,
                    binomial_name, captured_at,
                    location, camera, width, height, file_size, modified_at,
                    longitude, latitude, exif_json, thumbnail_path, status
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                _photo_values(record),
            )
            return int(cursor.lastrowid)

    def update_photo(self, photo_id: int, record: PhotoRecord) -> None:
        with self._conn:
            self._conn.execute(
                """
                UPDATE photos
                SET root = ?,
                    relative_path = ?,
                    parent_dir = ?,
                    path_depth = ?,
                    filename = ?,
                    binomial_name = ?,
                    captured_at = ?,
                    location = ?,
                    camera = ?,
                    width = ?,
                    height = ?,
                    file_size = ?,
                    modified_at = ?,
                    longitude = ?,
                    latitude = ?,
                    exif_json = ?,
                    thumbnail_path = ?,
                    status = ?
                WHERE photo_id = ?
                """,
                (
                    record.root,
                    record.relative_path,
                    record.parent_dir,
                    record.path_depth,
                    record.filename,
                    record.binomial_name,
                    record.captured_at,
                    record.location,
                    record.camera,
                    record.width,
                    record.height,
                    record.file_size,
                    record.modified_at,
                    record.longitude,
                    record.latitude,
                    record.exif_json,
                    record.thumbnail_path,
                    record.status,
                    photo_id,
                ),
            )

    def set_status(self, photo_id: int, status: str) -> None:
        with self._conn:
            self._conn.execute(
                """
                UPDATE photos
                SET status = ?
                WHERE photo_id = ?
                """,
                (status, photo_id),
            )

    def set_other_roots_unchanged(self, root: str) -> int:
        with self._conn:
            cursor = self._conn.execute(
                """
                UPDATE photos
                SET status = ?
                WHERE root != ? AND status != ?
                """,
                (STATUS_UNCHANGED, root, STATUS_DELETED),
            )
        return cursor.rowcount

    def set_thumbnail_path(self, photo_id: int, thumbnail_path: str) -> None:
        with self._conn:
            self._conn.execute(
                """
                UPDATE photos
                SET thumbnail_path = ?
                WHERE photo_id = ?
                """,
                (thumbnail_path, photo_id),
            )

    def get_photo_by_path(self, root: str, relative_path: str) -> dict | None:
        row = self._conn.execute(
            """
            SELECT * FROM photos
            WHERE root = ? AND relative_path = ?
            """,
            (root, relative_path),
        ).fetchone()
        return dict(row) if row else None

    def get_photo_by_id(self, photo_id: int) -> dict | None:
        row = self._conn.execute(
            "SELECT * FROM photos WHERE photo_id = ?",
            (photo_id,),
        ).fetchone()
        return dict(row) if row else None

    def list_photos(self) -> list[dict]:
        rows = self._conn.execute(
            "SELECT * FROM photos ORDER BY photo_id"
        ).fetchall()
        return [dict(row) for row in rows]

    def list_changed_photos(self) -> list[dict]:
        rows = self._conn.execute(
            """
            SELECT * FROM photos
            WHERE status IN (?, ?)
            ORDER BY photo_id
            """,
            (STATUS_UPDATED, STATUS_NEW),
        ).fetchall()
        return [dict(row) for row in rows]

    def list_photos_for_root(self, root: str) -> list[dict]:
        rows = self._conn.execute(
            "SELECT * FROM photos WHERE root = ? ORDER BY relative_path",
            (root,),
        ).fetchall()
        return [dict(row) for row in rows]

    def refresh_directories(self) -> None:
        roots = self._conn.execute(
            "SELECT DISTINCT root FROM photos ORDER BY root"
        ).fetchall()
        self.refresh_directories_for_roots([row["root"] for row in roots])

    def refresh_directories_for_roots(self, roots: list[str]) -> None:
        for root in roots:
            rows = self._conn.execute(
                """
                SELECT relative_path FROM photos
                WHERE root = ? AND status != ?
                """,
                (root, STATUS_DELETED),
            ).fetchall()
            directories = _directory_rows_for_paths(
                root,
                [row["relative_path"] for row in rows],
            )
            with self._conn:
                self._conn.execute("DELETE FROM photos_dir WHERE root = ?", (root,))
                self._conn.executemany(
                    """
                    INSERT INTO photos_dir (
                        root, relative_dir, parent_dir, name, path_depth
                    )
                    VALUES (?, ?, ?, ?, ?)
                    """,
                    directories,
                )

    def list_directory(self, root: str, relative_dir: str = "") -> dict:
        directory = _normalize_relative_path(relative_dir)
        dir_rows = self._conn.execute(
            """
            SELECT name FROM photos_dir
            WHERE root = ? AND parent_dir = ?
            ORDER BY name
            """,
            (root, directory),
        ).fetchall()
        file_rows = self._conn.execute(
            """
            SELECT * FROM photos
            WHERE root = ?
              AND status != ?
              AND parent_dir = ?
            ORDER BY filename
            """,
            (root, STATUS_DELETED, directory),
        ).fetchall()

        return {
            "root": root,
            "relative_dir": directory,
            "directories": [row["name"] for row in dir_rows],
            "files": [dict(row) for row in file_rows],
        }

    def count_directory_items(self, root: str, relative_dir: str = "") -> dict:
        directory = _normalize_relative_path(relative_dir)
        dir_row = self._conn.execute(
            """
            SELECT COUNT(*) AS count FROM photos_dir
            WHERE root = ? AND parent_dir = ?
            """,
            (root, directory),
        ).fetchone()
        file_row = self._conn.execute(
            """
            SELECT COUNT(*) AS count FROM photos
            WHERE root = ?
              AND status != ?
              AND parent_dir = ?
            """,
            (root, STATUS_DELETED, directory),
        ).fetchone()
        return {
            "directory_count": int(dir_row["count"]),
            "file_count": int(file_row["count"]),
        }

    def list_directory_dirs_page(
        self,
        root: str,
        relative_dir: str = "",
        after_name: str | None = None,
        limit: int = 100,
    ) -> list[str]:
        directory = _normalize_relative_path(relative_dir)
        if after_name is None:
            rows = self._conn.execute(
                """
                SELECT name FROM photos_dir
                WHERE root = ? AND parent_dir = ?
                ORDER BY name
                LIMIT ?
                """,
                (root, directory, limit),
            ).fetchall()
        else:
            rows = self._conn.execute(
                """
                SELECT name FROM photos_dir
                WHERE root = ? AND parent_dir = ? AND name > ?
                ORDER BY name
                LIMIT ?
                """,
                (root, directory, after_name, limit),
            ).fetchall()
        return [row["name"] for row in rows]

    def list_directory_files_page(
        self,
        root: str,
        relative_dir: str = "",
        after_filename: str | None = None,
        after_photo_id: int | None = None,
        limit: int = 100,
    ) -> list[dict]:
        directory = _normalize_relative_path(relative_dir)
        if after_filename is None or after_photo_id is None:
            rows = self._conn.execute(
                """
                SELECT * FROM photos
                WHERE root = ?
                  AND status != ?
                  AND parent_dir = ?
                ORDER BY filename, photo_id
                LIMIT ?
                """,
                (root, STATUS_DELETED, directory, limit),
            ).fetchall()
        else:
            rows = self._conn.execute(
                """
                SELECT * FROM photos
                WHERE root = ?
                  AND status != ?
                  AND parent_dir = ?
                  AND (
                    filename > ?
                    OR (filename = ? AND photo_id > ?)
                  )
                ORDER BY filename, photo_id
                LIMIT ?
                """,
                (
                    root,
                    STATUS_DELETED,
                    directory,
                    after_filename,
                    after_filename,
                    after_photo_id,
                    limit,
                ),
            ).fetchall()
        return [dict(row) for row in rows]

    def _backfill_photo_paths(self) -> None:
        rows = self._conn.execute(
            "SELECT photo_id, relative_path FROM photos"
        ).fetchall()
        self._conn.executemany(
            """
            UPDATE photos
            SET parent_dir = ?, path_depth = ?
            WHERE photo_id = ?
            """,
            (
                (
                    _parent_dir(row["relative_path"]),
                    _path_depth(_parent_dir(row["relative_path"])),
                    row["photo_id"],
                )
                for row in rows
            ),
        )


def _photo_values(record: PhotoRecord) -> tuple:
    return (
        record.root,
        record.relative_path,
        record.parent_dir,
        record.path_depth,
        record.filename,
        record.binomial_name,
        record.captured_at,
        record.location,
        record.camera,
        record.width,
        record.height,
        record.file_size,
        record.modified_at,
        record.longitude,
        record.latitude,
        record.exif_json,
        record.thumbnail_path,
        record.status,
    )


def _normalize_relative_path(path: str | Path) -> str:
    text = Path(path).as_posix().strip("/")
    return "" if text == "." else text


def _parent_dir(relative_path: str) -> str:
    parent = Path(relative_path).parent.as_posix()
    return "" if parent == "." else parent


def _path_depth(relative_dir: str) -> int:
    normalized = _normalize_relative_path(relative_dir)
    return 0 if not normalized else len(normalized.split("/"))


def _directory_rows_for_paths(
    root: str,
    relative_paths: list[str],
) -> list[tuple[str, str, str, str, int]]:
    directories: dict[str, tuple[str, str, str, str, int]] = {}
    for relative_path in relative_paths:
        parent_dir = _parent_dir(relative_path)
        if not parent_dir:
            continue

        parts = parent_dir.split("/")
        for index, name in enumerate(parts):
            relative_dir = "/".join(parts[: index + 1])
            parent = "/".join(parts[:index])
            directories[relative_dir] = (
                root,
                relative_dir,
                parent,
                name,
                index + 1,
            )
    return [directories[key] for key in sorted(directories)]


def _now() -> str:
    return datetime.now().isoformat(sep=" ", timespec="microseconds")
