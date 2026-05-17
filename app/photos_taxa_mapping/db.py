"""SQLite access layer for photo-taxa mappings."""

from __future__ import annotations

import sqlite3
from datetime import datetime
from pathlib import Path

from app.photos.db import DEFAULT_DB_PATH


SPECIAL_UNMAPPED_TAXON_ID = 0

METADATA_TABLE = "photos_taxa_mapping_metadata"
MAPPING_TABLE = "photos_taxa_mapping"
SUBTREE_TABLE = "photos_taxa_mapping_taxa"


class PhotosTaxaMappingDatabase:
    """Photo-to-taxon mapping table operations."""

    def __init__(self, db_path: str | Path = DEFAULT_DB_PATH) -> None:
        self.db_path = Path(db_path)
        self.db_path.parent.mkdir(parents=True, exist_ok=True)
        self._conn = sqlite3.connect(self.db_path)
        self._conn.row_factory = sqlite3.Row
        self.init_schema()

    def __enter__(self) -> "PhotosTaxaMappingDatabase":
        return self

    def __exit__(self, *_exc: object) -> None:
        self.close()

    def close(self) -> None:
        self._conn.close()

    def init_schema(self) -> None:
        self._conn.execute(
            f"""
            CREATE TABLE IF NOT EXISTS {METADATA_TABLE} (
                last_synced_at TEXT,
                photos_last_synced_at TEXT,
                taxa_last_synced_at TEXT
            )
            """
        )
        self._conn.execute(
            f"""
            CREATE TABLE IF NOT EXISTS {MAPPING_TABLE} (
                photo_id INTEGER PRIMARY KEY,
                taxon_id INTEGER NOT NULL
            )
            """
        )
        self._conn.execute(
            f"""
            CREATE TABLE IF NOT EXISTS {SUBTREE_TABLE} (
                taxon_id INTEGER PRIMARY KEY,
                rank TEXT NOT NULL,
                name TEXT NOT NULL,
                parent_id INTEGER,
                binomial_name TEXT
            )
            """
        )
        self._conn.execute(
            f"CREATE INDEX IF NOT EXISTS idx_{MAPPING_TABLE}_taxon ON {MAPPING_TABLE}(taxon_id)"
        )
        self._conn.execute(
            f"CREATE INDEX IF NOT EXISTS idx_{SUBTREE_TABLE}_parent ON {SUBTREE_TABLE}(parent_id)"
        )
        self._conn.execute(
            f"CREATE INDEX IF NOT EXISTS idx_{SUBTREE_TABLE}_binomial ON {SUBTREE_TABLE}(binomial_name)"
        )
        self._conn.execute(
            f"CREATE INDEX IF NOT EXISTS idx_{SUBTREE_TABLE}_name ON {SUBTREE_TABLE}(name)"
        )
        self._conn.commit()

    def clear_all(self) -> None:
        with self._conn:
            self._conn.execute(f"DELETE FROM {MAPPING_TABLE}")
            self._conn.execute(f"DELETE FROM {SUBTREE_TABLE}")

    def upsert_mapping(self, photo_id: int, taxon_id: int) -> None:
        with self._conn:
            self._conn.execute(
                f"""
                INSERT INTO {MAPPING_TABLE} (photo_id, taxon_id)
                VALUES (?, ?)
                ON CONFLICT(photo_id) DO UPDATE SET
                    taxon_id = excluded.taxon_id
                """,
                (photo_id, taxon_id),
            )

    def upsert_subtree_taxon(self, taxon: dict) -> None:
        with self._conn:
            self._conn.execute(
                f"""
                INSERT INTO {SUBTREE_TABLE} (
                    taxon_id, rank, name, parent_id, binomial_name
                )
                VALUES (?, ?, ?, ?, ?)
                ON CONFLICT(taxon_id) DO UPDATE SET
                    rank = excluded.rank,
                    name = excluded.name,
                    parent_id = excluded.parent_id,
                    binomial_name = excluded.binomial_name
                """,
                (
                    taxon["taxon_id"],
                    taxon["rank"],
                    taxon["name"],
                    taxon["parent_id"],
                    taxon["binomial_name"],
                ),
            )

    def get_subtree_taxon_by_id(self, taxon_id: int) -> dict | None:
        row = self._conn.execute(
            f"SELECT * FROM {SUBTREE_TABLE} WHERE taxon_id = ?",
            (taxon_id,),
        ).fetchone()
        return dict(row) if row else None

    def get_subtree_taxon_by_binomial(self, binomial_name: str) -> dict | None:
        row = self._conn.execute(
            f"""
            SELECT * FROM {SUBTREE_TABLE}
            WHERE binomial_name = ?
            ORDER BY taxon_id
            """,
            (binomial_name,),
        ).fetchone()
        return dict(row) if row else None

    def get_subtree_taxa_by_name(self, name: str) -> list[dict]:
        rows = self._conn.execute(
            f"""
            SELECT * FROM {SUBTREE_TABLE}
            WHERE name = ?
            ORDER BY taxon_id
            """,
            (name,),
        ).fetchall()
        return [dict(row) for row in rows]

    def save_metadata(
        self,
        photos_last_synced_at: str | None,
        taxa_last_synced_at: str | None,
    ) -> None:
        with self._conn:
            self._conn.execute(f"DELETE FROM {METADATA_TABLE}")
            self._conn.execute(
                f"""
                INSERT INTO {METADATA_TABLE} (
                    last_synced_at, photos_last_synced_at, taxa_last_synced_at
                )
                VALUES (?, ?, ?)
                """,
                (
                    datetime.now().isoformat(sep=" ", timespec="microseconds"),
                    photos_last_synced_at,
                    taxa_last_synced_at,
                ),
            )

    def get_metadata(self) -> dict:
        row = self._conn.execute(
            f"""
            SELECT last_synced_at, photos_last_synced_at, taxa_last_synced_at
            FROM {METADATA_TABLE}
            LIMIT 1
            """
        ).fetchone()
        return dict(row) if row else {
            "last_synced_at": None,
            "photos_last_synced_at": None,
            "taxa_last_synced_at": None,
        }

    def export_rows(self, table_name: str) -> tuple[list[str], list[dict]]:
        valid = {METADATA_TABLE, MAPPING_TABLE, SUBTREE_TABLE}
        if table_name not in valid:
            raise ValueError(
                f"table_name must be one of {', '.join(sorted(valid))}"
            )
        fieldnames = [
            column["name"]
            for column in self._conn.execute(f"PRAGMA table_info({table_name})")
        ]
        rows = self._conn.execute(f"SELECT * FROM {table_name}").fetchall()
        return fieldnames, [dict(row) for row in rows]

    def photo_ids_for_taxon(self, taxon_id: int) -> list[int]:
        rows = self._conn.execute(
            f"""
            SELECT photo_id FROM {MAPPING_TABLE}
            WHERE taxon_id = ?
            ORDER BY photo_id
            """,
            (taxon_id,),
        ).fetchall()
        return [int(row["photo_id"]) for row in rows]

    def children_for_taxon(self, taxon_id: int | None) -> list[dict]:
        if taxon_id is None:
            rows = self._conn.execute(
                f"""
                SELECT * FROM {SUBTREE_TABLE}
                WHERE parent_id IS NULL AND rank = 'ordo'
                ORDER BY name, taxon_id
                """
            ).fetchall()
        else:
            rows = self._conn.execute(
                f"""
                SELECT * FROM {SUBTREE_TABLE}
                WHERE parent_id = ?
                ORDER BY rank, name, taxon_id
                """,
                (taxon_id,),
            ).fetchall()
        return [dict(row) for row in rows]

