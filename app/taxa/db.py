"""SQLite access layer for taxa adjacency lists."""

from __future__ import annotations

import sqlite3
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

from app.photos.db import DEFAULT_DB_PATH


@dataclass(frozen=True)
class TaxonRecord:
    rank: str
    name: str
    parent_id: int | None = None
    binomial_name: str | None = None


@dataclass(frozen=True)
class TaxaMetadata:
    knowledge_base_path: str | None = None
    knowledge_base_size: int | None = None
    knowledge_base_modified_at: str | None = None
    last_synced_at: str | None = None


class TaxaDatabase:
    """Taxa table operations and query interface."""

    def __init__(self, db_path: str | Path = DEFAULT_DB_PATH) -> None:
        self.db_path = Path(db_path)
        self.db_path.parent.mkdir(parents=True, exist_ok=True)
        self._conn = sqlite3.connect(self.db_path)
        self._conn.row_factory = sqlite3.Row
        self.init_schema()

    def __enter__(self) -> "TaxaDatabase":
        return self

    def __exit__(self, *_exc: object) -> None:
        self.close()

    def close(self) -> None:
        self._conn.close()

    def init_schema(self) -> None:
        self._ensure_taxa_table()
        self._ensure_metadata_table()
        self._conn.execute(
            """
            CREATE INDEX IF NOT EXISTS idx_taxa_parent
            ON taxa(parent_id)
            """
        )
        self._conn.execute(
            """
            CREATE INDEX IF NOT EXISTS idx_taxa_binomial
            ON taxa(binomial_name)
            """
        )
        self._conn.commit()

    def _ensure_taxa_table(self) -> None:
        row = self._conn.execute(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'taxa'"
        ).fetchone()
        if row is None:
            self._create_taxa_table("taxa")
            self._conn.commit()
            return

        columns = [
            column["name"]
            for column in self._conn.execute("PRAGMA table_info(taxa)").fetchall()
        ]
        desired = ["taxon_id", "rank", "name", "parent_id", "binomial_name"]
        if columns == desired:
            return

        with self._conn:
            self._conn.execute("DROP TABLE IF EXISTS taxa_new")
            self._create_taxa_table("taxa_new")
            common = [column for column in desired if column in columns]
            self._conn.execute(
                f"""
                INSERT INTO taxa_new ({", ".join(common)})
                SELECT {", ".join(common)} FROM taxa
                """
            )
            self._conn.execute("DROP TABLE taxa")
            self._conn.execute("ALTER TABLE taxa_new RENAME TO taxa")

    def _ensure_metadata_table(self) -> None:
        row = self._conn.execute(
            """
            SELECT name FROM sqlite_master
            WHERE type = 'table' AND name = 'taxa_metadata'
            """
        ).fetchone()
        if row is None:
            self._create_metadata_table()
            return

        columns = [
            column["name"]
            for column in self._conn.execute("PRAGMA table_info(taxa_metadata)").fetchall()
        ]
        desired = [
            "knowledge_base_path",
            "knowledge_base_size",
            "knowledge_base_modified_at",
            "last_synced_at",
        ]
        if columns == desired:
            return

        existing = self._conn.execute("SELECT * FROM taxa_metadata LIMIT 1").fetchone()
        existing_data = dict(existing) if existing else {}
        knowledge_base_path = existing_data.get("knowledge_base_path")
        knowledge_base_size = existing_data.get("knowledge_base_size")
        if knowledge_base_size is None and knowledge_base_path:
            path = Path(knowledge_base_path).expanduser()
            knowledge_base_size = path.stat().st_size if path.exists() else None

        with self._conn:
            self._conn.execute("DROP TABLE taxa_metadata")
            self._create_metadata_table()
            if existing_data:
                self._conn.execute(
                    """
                    INSERT INTO taxa_metadata (
                        knowledge_base_path, knowledge_base_size,
                        knowledge_base_modified_at, last_synced_at
                    )
                    VALUES (?, ?, ?, ?)
                    """,
                    (
                        knowledge_base_path,
                        knowledge_base_size,
                        existing_data.get("knowledge_base_modified_at"),
                        existing_data.get("last_synced_at"),
                    ),
                )

    def _create_taxa_table(self, table_name: str) -> None:
        self._conn.execute(
            f"""
            CREATE TABLE {table_name} (
                taxon_id INTEGER PRIMARY KEY AUTOINCREMENT,
                rank TEXT NOT NULL,
                name TEXT NOT NULL,
                parent_id INTEGER REFERENCES taxa(taxon_id) ON DELETE CASCADE,
                binomial_name TEXT
            )
            """
        )

    def _create_metadata_table(self) -> None:
        self._conn.execute(
            """
            CREATE TABLE taxa_metadata (
                knowledge_base_path TEXT,
                knowledge_base_size INTEGER,
                knowledge_base_modified_at TEXT,
                last_synced_at TEXT
            )
            """
        )

    def insert_taxon(self, record: TaxonRecord) -> int:
        with self._conn:
            cursor = self._conn.execute(
                """
                INSERT INTO taxa (rank, name, parent_id, binomial_name)
                VALUES (?, ?, ?, ?)
                """,
                (
                    record.rank,
                    record.name,
                    record.parent_id,
                    record.binomial_name,
                ),
            )
            return int(cursor.lastrowid)

    def update_or_insert_by_binomial(self, record: TaxonRecord) -> int:
        if record.binomial_name:
            row = self._conn.execute(
                "SELECT taxon_id FROM taxa WHERE binomial_name = ? ORDER BY taxon_id",
                (record.binomial_name,),
            ).fetchone()
            if row:
                taxon_id = int(row["taxon_id"])
                with self._conn:
                    self._conn.execute(
                        """
                        UPDATE taxa
                        SET rank = ?, name = ?, parent_id = ?
                        WHERE taxon_id = ?
                        """,
                        (record.rank, record.name, record.parent_id, taxon_id),
                    )
                return taxon_id

        return self.insert_taxon(record)

    def clear_taxa(self) -> None:
        with self._conn:
            self._conn.execute("DELETE FROM taxa")
            self._conn.execute("DELETE FROM sqlite_sequence WHERE name = 'taxa'")

    def save_metadata(
        self,
        knowledge_base_path: str | Path,
        knowledge_base_size: int,
        knowledge_base_modified_at: str,
    ) -> None:
        with self._conn:
            self._conn.execute("DELETE FROM taxa_metadata")
            self._conn.execute(
                """
                INSERT INTO taxa_metadata (
                    knowledge_base_path, knowledge_base_size,
                    knowledge_base_modified_at, last_synced_at
                )
                VALUES (?, ?, ?, ?)
                """,
                (
                    str(Path(knowledge_base_path).expanduser()),
                    knowledge_base_size,
                    knowledge_base_modified_at,
                    datetime.now().isoformat(sep=" ", timespec="microseconds"),
                ),
            )

    def save_knowledge_base_path(self, knowledge_base_path: str | Path | None) -> TaxaMetadata:
        current = self.get_metadata()
        normalized_path = (
            str(Path(knowledge_base_path).expanduser())
            if knowledge_base_path
            else None
        )
        same_path = normalized_path == current.knowledge_base_path
        with self._conn:
            self._conn.execute("DELETE FROM taxa_metadata")
            self._conn.execute(
                """
                INSERT INTO taxa_metadata (
                    knowledge_base_path, knowledge_base_size,
                    knowledge_base_modified_at, last_synced_at
                )
                VALUES (?, ?, ?, ?)
                """,
                (
                    normalized_path,
                    current.knowledge_base_size if same_path else None,
                    current.knowledge_base_modified_at if same_path else None,
                    current.last_synced_at,
                ),
            )
        return self.get_metadata()

    def get_metadata(self) -> TaxaMetadata:
        row = self._conn.execute(
            """
            SELECT
                knowledge_base_path,
                knowledge_base_size,
                knowledge_base_modified_at,
                last_synced_at
            FROM taxa_metadata
            LIMIT 1
            """
        ).fetchone()
        if row is None:
            return TaxaMetadata()
        return TaxaMetadata(**dict(row))

    def export_rows(self, table_name: str) -> tuple[list[str], list[dict]]:
        if table_name not in {"taxa", "taxa_metadata"}:
            raise ValueError("table_name must be 'taxa' or 'taxa_metadata'")
        fieldnames = [
            column["name"]
            for column in self._conn.execute(f"PRAGMA table_info({table_name})")
        ]
        rows = self._conn.execute(f"SELECT * FROM {table_name}").fetchall()
        return fieldnames, [dict(row) for row in rows]

    def get_taxon(self, taxon_id: int) -> dict | None:
        row = self._conn.execute(
            "SELECT * FROM taxa WHERE taxon_id = ?",
            (taxon_id,),
        ).fetchone()
        return dict(row) if row else None

    def get_taxon_by_binomial(self, binomial_name: str) -> dict | None:
        row = self._conn.execute(
            "SELECT * FROM taxa WHERE binomial_name = ? ORDER BY taxon_id",
            (binomial_name,),
        ).fetchone()
        return dict(row) if row else None

    def children_of(self, parent_id: int | None) -> list[dict]:
        if parent_id is None:
            rows = self._conn.execute(
                "SELECT * FROM taxa WHERE parent_id IS NULL ORDER BY rank, name"
            ).fetchall()
        else:
            rows = self._conn.execute(
                "SELECT * FROM taxa WHERE parent_id = ? ORDER BY rank, name",
                (parent_id,),
            ).fetchall()
        return [dict(row) for row in rows]

    def list_taxa(self) -> list[dict]:
        rows = self._conn.execute(
            """
            SELECT taxon_id, rank, name, parent_id, binomial_name
            FROM taxa
            ORDER BY taxon_id
            """
        ).fetchall()
        return [dict(row) for row in rows]

    def lineage(self, taxon_id: int) -> list[dict]:
        lineage = []
        current_id: int | None = taxon_id
        while current_id is not None:
            taxon = self.get_taxon(current_id)
            if taxon is None:
                break
            lineage.append(taxon)
            current_id = taxon["parent_id"]
        return list(reversed(lineage))

    def count_taxa(self) -> int:
        row = self._conn.execute("SELECT COUNT(*) AS count FROM taxa").fetchone()
        return int(row["count"])
