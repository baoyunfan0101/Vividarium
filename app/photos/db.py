"""SQLite access layer for photo metadata."""

from __future__ import annotations

import sqlite3
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path


DEFAULT_DB_PATH = Path("data/phytoindex.db")

STATUS_UNCHANGED = "unchanged"
STATUS_DELETED = "deleted"
STATUS_UPDATED = "updated"
STATUS_NEW = "new"


@dataclass(frozen=True)
class PhotoRecord:
    root: str
    relative_path: str
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
        self._ensure_metadata_table()
        self._conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_photos_root_path ON photos(root, relative_path)"
        )
        self._conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_photos_status ON photos(status)"
        )
        self._conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_photos_binomial_name ON photos(binomial_name)"
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
            SELECT root, last_synced_at, sort_order
            FROM photos_metadata
            ORDER BY sort_order, root
            """
        ).fetchall()
        return [dict(row) for row in rows]

    def _next_sort_order(self) -> int:
        row = self._conn.execute(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 AS next_order FROM photos_metadata"
        ).fetchone()
        return int(row["next_order"])

    def export_rows(self, table_name: str) -> tuple[list[str], list[dict]]:
        if table_name not in {"photos", "photos_metadata"}:
            raise ValueError("table_name must be 'photos' or 'photos_metadata'")
        fieldnames = [
            column["name"]
            for column in self._conn.execute(f"PRAGMA table_info({table_name})")
        ]
        rows = self._conn.execute(f"SELECT * FROM {table_name}").fetchall()
        return fieldnames, [dict(row) for row in rows]

    def clear_photos(self) -> None:
        with self._conn:
            self._conn.execute("DELETE FROM photos")
            self._conn.execute("DELETE FROM sqlite_sequence WHERE name = 'photos'")

    def clear_photos_for_roots(self, roots: list[str]) -> None:
        with self._conn:
            self._conn.executemany(
                "DELETE FROM photos WHERE root = ?",
                ((root,) for root in roots),
            )

    def insert_photo(self, record: PhotoRecord) -> int:
        with self._conn:
            cursor = self._conn.execute(
                """
                INSERT INTO photos (
                    root, relative_path, filename, binomial_name, captured_at,
                    location, camera, width, height, file_size, modified_at,
                    longitude, latitude, exif_json, thumbnail_path, status
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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

    def list_directory(self, root: str, relative_dir: str = "") -> dict:
        directory = _normalize_relative_path(relative_dir)
        prefix = f"{directory}/" if directory else ""
        rows = self._conn.execute(
            """
            SELECT * FROM photos
            WHERE root = ?
              AND status != ?
              AND relative_path LIKE ?
            ORDER BY relative_path
            """,
            (root, STATUS_DELETED, f"{prefix}%"),
        ).fetchall()

        dirs: set[str] = set()
        files: list[dict] = []
        for row in rows:
            photo = dict(row)
            rest = photo["relative_path"][len(prefix) :]
            if not rest:
                continue
            first, _, remainder = rest.partition("/")
            if remainder:
                dirs.add(first)
            else:
                files.append(photo)

        return {
            "root": root,
            "relative_dir": directory,
            "directories": sorted(dirs),
            "files": files,
        }


def _photo_values(record: PhotoRecord) -> tuple:
    return (
        record.root,
        record.relative_path,
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


def _now() -> str:
    return datetime.now().isoformat(sep=" ", timespec="microseconds")
