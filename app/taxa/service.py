"""Public service API for importing and querying taxa."""

from __future__ import annotations

import csv
from datetime import datetime
from pathlib import Path
from typing import Callable, Iterable

from app.photos.db import DEFAULT_DB_PATH

from .db import TaxaDatabase, TaxonRecord
from .parser import PLANTS_SHEET_NAME, TaxaRow, read_taxa_rows


def update_taxa(
    knowledge_base_path: str | Path | None = None,
    db_path: str | Path = DEFAULT_DB_PATH,
    max_rows: int | None = None,
    progress: Callable[[int, int | None, str | None], None] | None = None,
) -> dict[str, int | str]:
    """Update taxa, keeping taxon_id stable for existing binomial names."""

    with TaxaDatabase(db_path) as db:
        workbook_path = _resolve_knowledge_base_path(db, knowledge_base_path)
        rows = list(read_taxa_rows(workbook_path, max_rows=max_rows))
        _report_progress(progress, 0, len(rows), "Updating taxa")
        changed = _import_rows(rows, db, preserve_binomial_ids=True, progress=progress)
        db.save_metadata(
            workbook_path,
            _file_size(workbook_path),
            _file_modified_at(workbook_path),
        )
        total_taxa = db.count_taxa()

    return _summary(workbook_path, len(rows), changed, total_taxa)


def rebuild_taxa(
    knowledge_base_path: str | Path | None = None,
    db_path: str | Path = DEFAULT_DB_PATH,
    max_rows: int | None = None,
    progress: Callable[[int, int | None, str | None], None] | None = None,
) -> dict[str, int | str]:
    """Rebuild taxa from scratch, allowing taxon_id values to change."""

    with TaxaDatabase(db_path) as db:
        workbook_path = _resolve_knowledge_base_path(db, knowledge_base_path)
        rows = list(read_taxa_rows(workbook_path, max_rows=max_rows))
        db.clear_taxa()
        _report_progress(progress, 0, len(rows), "Rebuilding taxa")
        changed = _import_rows(rows, db, preserve_binomial_ids=False, progress=progress)
        db.save_metadata(
            workbook_path,
            _file_size(workbook_path),
            _file_modified_at(workbook_path),
        )
        total_taxa = db.count_taxa()

    return _summary(workbook_path, len(rows), changed, total_taxa)


def get_taxon_by_id(taxon_id: int, db_path: str | Path = DEFAULT_DB_PATH) -> dict | None:
    with TaxaDatabase(db_path) as db:
        return db.get_taxon(taxon_id)


def get_taxon_by_binomial(
    binomial_name: str,
    db_path: str | Path = DEFAULT_DB_PATH,
) -> dict | None:
    with TaxaDatabase(db_path) as db:
        return db.get_taxon_by_binomial(binomial_name)


def get_latest_update(db_path: str | Path = DEFAULT_DB_PATH) -> dict:
    with TaxaDatabase(db_path) as db:
        metadata = db.get_metadata().__dict__
        metadata["taxa_count"] = db.count_taxa()
        return metadata


def save_knowledge_base_path(
    knowledge_base_path: str | Path | None,
    db_path: str | Path = DEFAULT_DB_PATH,
) -> dict:
    with TaxaDatabase(db_path) as db:
        metadata = db.save_knowledge_base_path(knowledge_base_path).__dict__
        metadata["taxa_count"] = db.count_taxa()
        return metadata


def export_table_csv(
    table_name: str,
    output_path: str | Path,
    db_path: str | Path = DEFAULT_DB_PATH,
) -> int:
    """Export one taxa module table to CSV."""

    if table_name not in {"taxa", "taxa_metadata"}:
        raise ValueError("table_name must be 'taxa' or 'taxa_metadata'")

    output = Path(output_path)
    output.parent.mkdir(parents=True, exist_ok=True)

    with TaxaDatabase(db_path) as db:
        fieldnames, rows = db.export_rows(table_name)

    with output.open("w", newline="", encoding="utf-8-sig") as file:
        writer = csv.DictWriter(file, fieldnames=fieldnames)
        writer.writeheader()
        writer.writerows(rows)

    return len(rows)


def _import_rows(
    rows: Iterable[TaxaRow],
    db: TaxaDatabase,
    preserve_binomial_ids: bool,
    progress: Callable[[int, int | None, str | None], None] | None = None,
) -> int:
    row_list = list(rows)
    current_parent_by_rank: dict[str, int | None] = {
        "ordo": None,
        "familia": None,
        "genus": None,
        "species": None,
    }
    changed = 0

    for index, row in enumerate(row_list, start=1):
        if row.ordo:
            current_parent_by_rank["ordo"] = _save_taxon(
                db,
                TaxonRecord(
                    rank="ordo",
                    name=row.ordo,
                    parent_id=None,
                    binomial_name=row.binomial_name,
                ),
                preserve_binomial_ids,
            )
            current_parent_by_rank["familia"] = None
            current_parent_by_rank["genus"] = None
            current_parent_by_rank["species"] = None
            changed += 1

        if row.familia:
            current_parent_by_rank["familia"] = _save_taxon(
                db,
                TaxonRecord(
                    rank="familia",
                    name=row.familia,
                    parent_id=current_parent_by_rank["ordo"],
                    binomial_name=row.binomial_name,
                ),
                preserve_binomial_ids,
            )
            current_parent_by_rank["genus"] = None
            current_parent_by_rank["species"] = None
            changed += 1

        if row.genus:
            current_parent_by_rank["genus"] = _save_taxon(
                db,
                TaxonRecord(
                    rank="genus",
                    name=row.genus,
                    parent_id=_nearest_parent(
                        current_parent_by_rank,
                        "familia",
                        "ordo",
                    ),
                    binomial_name=row.binomial_name,
                ),
                preserve_binomial_ids,
            )
            current_parent_by_rank["species"] = None
            changed += 1

        if row.species:
            current_parent_by_rank["species"] = _save_taxon(
                db,
                TaxonRecord(
                    rank="species",
                    name=row.species,
                    parent_id=_nearest_parent(
                        current_parent_by_rank,
                        "genus",
                        "familia",
                        "ordo",
                    ),
                    binomial_name=row.binomial_name,
                ),
                preserve_binomial_ids,
            )
            changed += 1
        _report_progress(progress, index, len(row_list), "Importing taxa")

    return changed


def _report_progress(
    progress: Callable[[int, int | None, str | None], None] | None,
    processed: int,
    total: int | None,
    message: str | None,
) -> None:
    if progress is not None:
        progress(processed, total, message)


def _save_taxon(
    db: TaxaDatabase,
    record: TaxonRecord,
    preserve_binomial_ids: bool,
) -> int:
    if preserve_binomial_ids:
        return db.update_or_insert_by_binomial(record)
    return db.insert_taxon(record)


def _nearest_parent(
    current_parent_by_rank: dict[str, int | None],
    *ranks: str,
) -> int | None:
    for rank in ranks:
        parent_id = current_parent_by_rank[rank]
        if parent_id is not None:
            return parent_id
    return None


def _resolve_knowledge_base_path(
    db: TaxaDatabase,
    knowledge_base_path: str | Path | None,
) -> Path:
    if knowledge_base_path is not None:
        return Path(knowledge_base_path).expanduser()

    recorded_path = db.get_metadata().knowledge_base_path
    if recorded_path:
        return Path(recorded_path).expanduser()

    raise ValueError("knowledge_base_path is required before one has been recorded")


def _file_modified_at(path: Path) -> str:
    return datetime.fromtimestamp(path.stat().st_mtime).isoformat(
        sep=" ",
        timespec="seconds",
    )


def _file_size(path: Path) -> int:
    return path.stat().st_size


def _summary(
    workbook_path: Path,
    rows_read: int,
    taxa_changed: int,
    total_taxa: int,
) -> dict[str, int | str]:
    return {
        "knowledge_base_path": str(workbook_path),
        "knowledge_base_size": _file_size(workbook_path),
        "knowledge_base_modified_at": _file_modified_at(workbook_path),
        "sheet": PLANTS_SHEET_NAME,
        "rows_read": rows_read,
        "taxa_changed": taxa_changed,
        "total_taxa": total_taxa,
    }
