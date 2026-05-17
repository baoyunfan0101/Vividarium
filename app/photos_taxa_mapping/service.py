"""Public service API for photo-to-taxa mapping."""

from __future__ import annotations

import csv
from pathlib import Path

from app.photos import get_latest_update as get_photos_latest_update
from app.photos import list_changed_photos, list_photos
from app.photos.db import DEFAULT_DB_PATH
from app.taxa import get_latest_update as get_taxa_latest_update
from app.taxa.db import TaxaDatabase

from .db import (
    MAPPING_TABLE,
    METADATA_TABLE,
    SPECIAL_UNMAPPED_TAXON_ID,
    SUBTREE_TABLE,
    PhotosTaxaMappingDatabase,
)


def update_mapping(db_path: str | Path = DEFAULT_DB_PATH) -> dict:
    """Map photos with status updated/new."""

    photos = list_changed_photos(db_path=db_path)
    return _sync_photos(photos, db_path=db_path, rebuild=False)


def rebuild_mapping(db_path: str | Path = DEFAULT_DB_PATH) -> dict:
    """Rebuild mapping from all photo rows."""

    photos = list_photos(db_path=db_path)
    return _sync_photos(photos, db_path=db_path, rebuild=True)


def get_by_taxon_id(
    taxon_id: int | None,
    db_path: str | Path = DEFAULT_DB_PATH,
) -> dict:
    with PhotosTaxaMappingDatabase(db_path) as db:
        return _result_for_taxon_id(db, taxon_id)


def get_by_binomial_name(
    binomial_name: str,
    db_path: str | Path = DEFAULT_DB_PATH,
) -> dict:
    with PhotosTaxaMappingDatabase(db_path) as db:
        taxon = db.get_subtree_taxon_by_binomial(binomial_name)
        if taxon is None:
            return {"taxon": None, "photo_ids": [], "children": []}
        return _result_for_taxon_id(db, int(taxon["taxon_id"]))


def get_by_name(
    name: str,
    db_path: str | Path = DEFAULT_DB_PATH,
) -> dict:
    with PhotosTaxaMappingDatabase(db_path) as db:
        taxa = db.get_subtree_taxa_by_name(name)
        if not taxa:
            return {"taxon": None, "photo_ids": [], "children": []}
        return _result_for_taxon_id(db, int(taxa[0]["taxon_id"]))


def get_latest_update(db_path: str | Path = DEFAULT_DB_PATH) -> dict:
    with PhotosTaxaMappingDatabase(db_path) as db:
        return db.get_metadata()


def export_table_csv(
    table_name: str,
    output_path: str | Path,
    db_path: str | Path = DEFAULT_DB_PATH,
) -> int:
    output = Path(output_path)
    output.parent.mkdir(parents=True, exist_ok=True)

    with PhotosTaxaMappingDatabase(db_path) as db:
        fieldnames, rows = db.export_rows(table_name)

    with output.open("w", newline="", encoding="utf-8-sig") as file:
        writer = csv.DictWriter(file, fieldnames=fieldnames)
        writer.writeheader()
        writer.writerows(rows)

    return len(rows)


def _sync_photos(
    photos: list[dict],
    db_path: str | Path,
    rebuild: bool,
) -> dict:
    unmapped_photos = []
    mapped = 0

    with PhotosTaxaMappingDatabase(db_path) as mapping_db, TaxaDatabase(db_path) as taxa_db:
        if rebuild:
            mapping_db.clear_all()

        for photo in photos:
            taxon_id = _taxon_id_for_photo(photo, mapping_db, taxa_db)
            if taxon_id == SPECIAL_UNMAPPED_TAXON_ID:
                unmapped_photos.append(photo)
            mapping_db.upsert_mapping(int(photo["photo_id"]), taxon_id)
            mapped += 1

        mapping_db.save_metadata(
            _photos_last_synced_at(db_path),
            get_taxa_latest_update(db_path=db_path).get("last_synced_at"),
        )

    return {
        "processed": len(photos),
        "mapped": mapped,
        "unmapped": len(unmapped_photos),
        "unmapped_photos": unmapped_photos,
    }


def _taxon_id_for_photo(
    photo: dict,
    mapping_db: PhotosTaxaMappingDatabase,
    taxa_db: TaxaDatabase,
) -> int:
    binomial_name = photo.get("binomial_name")
    if not binomial_name:
        return SPECIAL_UNMAPPED_TAXON_ID

    subtree_taxon = mapping_db.get_subtree_taxon_by_binomial(binomial_name)
    if subtree_taxon:
        return int(subtree_taxon["taxon_id"])

    source_taxon = taxa_db.get_taxon_by_binomial(binomial_name)
    if source_taxon is None:
        return SPECIAL_UNMAPPED_TAXON_ID

    for taxon in taxa_db.lineage(int(source_taxon["taxon_id"])):
        mapping_db.upsert_subtree_taxon(taxon)
    return int(source_taxon["taxon_id"])


def _result_for_taxon_id(
    db: PhotosTaxaMappingDatabase,
    taxon_id: int | None,
) -> dict:
    taxon = None
    photo_ids: list[int] = []
    if taxon_id is not None:
        taxon = db.get_subtree_taxon_by_id(taxon_id)
        photo_ids = _photo_ids_for_taxon(db, taxon_id)
    return {
        "taxon": taxon,
        "photo_ids": photo_ids,
        "children": _children_for_taxon(db, taxon_id),
    }


def _photo_ids_for_taxon(
    db: PhotosTaxaMappingDatabase,
    taxon_id: int,
) -> list[int]:
    return db.photo_ids_for_taxon(taxon_id)


def _children_for_taxon(
    db: PhotosTaxaMappingDatabase,
    taxon_id: int | None,
) -> list[dict]:
    return db.children_for_taxon(taxon_id)


def _photos_last_synced_at(db_path: str | Path) -> str | None:
    updates = get_photos_latest_update(db_path=db_path)
    values = [row["last_synced_at"] for row in updates if row.get("last_synced_at")]
    return max(values) if values else None


__all_tables__ = [METADATA_TABLE, MAPPING_TABLE, SUBTREE_TABLE]
