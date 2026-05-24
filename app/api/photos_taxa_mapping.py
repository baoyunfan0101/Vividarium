"""Photo-to-taxa mapping API routes."""

from __future__ import annotations

from typing import Optional

from fastapi import APIRouter, HTTPException, Query
from pydantic import BaseModel

from app.operations import OperationBusyError, operation_manager
from app.photos import get_latest_update as get_photos_latest_update
from app.photos_taxa_mapping import (
    export_table_csv,
    get_by_binomial_name,
    get_by_name,
    get_by_taxon_id,
    get_latest_update,
    rebuild_mapping,
    suggest_taxa,
    update_mapping,
)
from app.photos_taxa_mapping.db import PhotosTaxaMappingDatabase
from app.taxa import get_latest_update as get_taxa_latest_update

from .utils import csv_download_response, iso_after


router = APIRouter(prefix="/mapping/photos-taxa", tags=["photos-taxa-mapping"])


class MappingSyncRequest(BaseModel):
    force: bool = False


class ExportRequest(BaseModel):
    table_name: str
    output_path: str


@router.get("/latest-update")
def api_get_latest_update() -> dict:
    return {"metadata": get_latest_update()}


@router.get("/root")
def api_get_mapping_root() -> dict:
    return get_by_taxon_id(None)


@router.get("/taxon/{taxon_id}")
def api_get_by_taxon_id(taxon_id: int) -> dict:
    return get_by_taxon_id(taxon_id)


@router.get("/search-binomial")
def api_get_by_binomial_name(binomial_name: str = Query(...)) -> dict:
    return get_by_binomial_name(binomial_name)


@router.get("/search")
def api_get_by_name(name: str = Query(...)) -> dict:
    return get_by_name(name)


@router.get("/suggest")
def api_suggest_taxa(
    query: str = Query(...),
    mode: str = Query("name"),
    limit: int = Query(10, ge=1, le=30),
) -> dict:
    if mode not in {"name", "binomial"}:
        raise HTTPException(status_code=400, detail="mode must be 'name' or 'binomial'")
    return {"suggestions": suggest_taxa(query, mode, limit)}


@router.post("/update")
def api_update_mapping(request: MappingSyncRequest) -> dict:
    confirmation = _mapping_confirmation()
    if confirmation and not request.force:
        return confirmation
    try:
        state = operation_manager.start(
            "mapping",
            "update",
            lambda: update_mapping(
                progress=lambda processed, total, message: operation_manager.progress(
                    "mapping",
                    processed,
                    total,
                    message,
                ),
            ),
        )
    except OperationBusyError as exc:
        raise HTTPException(
            status_code=409,
            detail={
                "code": "operation_busy",
                "module": exc.module,
                "blocked_by": exc.blocked_by,
            },
        ) from exc
    return {"operation": state}


@router.post("/rebuild")
def api_rebuild_mapping(request: MappingSyncRequest) -> dict:
    confirmation = _mapping_confirmation()
    if confirmation and not request.force:
        return confirmation
    try:
        state = operation_manager.start(
            "mapping",
            "rebuild",
            lambda: rebuild_mapping(
                progress=lambda processed, total, message: operation_manager.progress(
                    "mapping",
                    processed,
                    total,
                    message,
                ),
            ),
        )
    except OperationBusyError as exc:
        raise HTTPException(
            status_code=409,
            detail={
                "code": "operation_busy",
                "module": exc.module,
                "blocked_by": exc.blocked_by,
            },
        ) from exc
    return {"operation": state}


@router.post("/export")
def api_export_mapping(request: ExportRequest) -> dict:
    exported = export_table_csv(request.table_name, request.output_path)
    return {"exported": exported, "output_path": request.output_path}


@router.get("/export")
def api_download_mapping_table(table_name: str) -> object:
    with PhotosTaxaMappingDatabase() as db:
        fieldnames, rows = db.export_rows(table_name)
    return csv_download_response(f"{table_name}.csv", fieldnames, rows)


def _mapping_confirmation() -> Optional[dict]:
    photos_metadata = get_photos_latest_update()
    taxa_metadata = get_taxa_latest_update()
    photos_latest = max(
        (
            row["last_synced_at"]
            for row in photos_metadata
            if row.get("last_synced_at")
        ),
        default=None,
    )
    taxa_latest = taxa_metadata.get("last_synced_at")
    mapping_metadata = get_latest_update()

    inputs_unchanged = (
        mapping_metadata.get("last_synced_at") is not None
        and mapping_metadata.get("photos_last_synced_at") == photos_latest
        and mapping_metadata.get("taxa_last_synced_at") == taxa_latest
    )
    if inputs_unchanged:
        return {
            "needs_confirmation": True,
            "reason": "mapping_inputs_unchanged",
            "message": (
                "Photos and taxa appear unchanged since the last mapping sync. "
                "Are you sure you want to continue this update/rebuild anyway?"
            ),
            "photos_last_synced_at": photos_latest,
            "taxa_last_synced_at": taxa_latest,
            "mapping_last_synced_at": mapping_metadata.get("last_synced_at"),
        }

    if not iso_after(taxa_latest, photos_latest):
        return None

    return {
        "needs_confirmation": True,
        "reason": "taxa_newer_than_photos",
        "message": "Taxa were synced later than photos. Confirm before updating mapping.",
        "photos_last_synced_at": photos_latest,
        "taxa_last_synced_at": taxa_latest,
    }
