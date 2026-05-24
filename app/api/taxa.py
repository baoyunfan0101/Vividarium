"""Taxa API routes."""

from __future__ import annotations

from typing import Optional

from fastapi import APIRouter, HTTPException, Query
from pydantic import BaseModel

from app.operations import OperationBusyError, operation_manager
from app.taxa import (
    export_table_csv,
    get_latest_update,
    get_taxon_by_binomial,
    get_taxon_by_id,
    rebuild_taxa,
    save_knowledge_base_path,
    update_taxa,
)
from app.taxa.db import TaxaDatabase

from .utils import csv_download_response, file_signature


router = APIRouter(prefix="/taxa", tags=["taxa"])


class TaxaSyncRequest(BaseModel):
    knowledge_base_path: Optional[str] = None
    force: bool = False


class ExportRequest(BaseModel):
    table_name: str
    output_path: str


class KnowledgeBasePathRequest(BaseModel):
    knowledge_base_path: Optional[str] = None


@router.get("/latest-update")
def api_get_latest_update() -> dict:
    return {"metadata": get_latest_update()}


@router.put("/knowledge-base")
def api_save_knowledge_base_path(request: KnowledgeBasePathRequest) -> dict:
    return {"metadata": save_knowledge_base_path(request.knowledge_base_path)}


@router.get("/by-id/{taxon_id}")
def api_get_taxon_by_id(taxon_id: int) -> dict:
    taxon = get_taxon_by_id(taxon_id)
    if taxon is None:
        raise HTTPException(status_code=404, detail="taxon not found")
    return {"taxon": taxon}


@router.get("/by-binomial")
def api_get_taxon_by_binomial(binomial_name: str = Query(...)) -> dict:
    taxon = get_taxon_by_binomial(binomial_name)
    if taxon is None:
        raise HTTPException(status_code=404, detail="taxon not found")
    return {"taxon": taxon}


@router.post("/update")
def api_update_taxa(request: TaxaSyncRequest) -> dict:
    confirmation = _knowledge_base_confirmation(request.knowledge_base_path)
    if confirmation and not request.force:
        return confirmation
    try:
        state = operation_manager.start(
            "taxa",
            "update",
            lambda: update_taxa(
                request.knowledge_base_path,
                progress=lambda processed, total, message: operation_manager.progress(
                    "taxa",
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
def api_rebuild_taxa(request: TaxaSyncRequest) -> dict:
    confirmation = _knowledge_base_confirmation(request.knowledge_base_path)
    if confirmation and not request.force:
        return confirmation
    try:
        state = operation_manager.start(
            "taxa",
            "rebuild",
            lambda: rebuild_taxa(
                request.knowledge_base_path,
                progress=lambda processed, total, message: operation_manager.progress(
                    "taxa",
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
def api_export_taxa(request: ExportRequest) -> dict:
    exported = export_table_csv(request.table_name, request.output_path)
    return {"exported": exported, "output_path": request.output_path}


@router.get("/export")
def api_download_taxa_table(table_name: str) -> object:
    with TaxaDatabase() as db:
        fieldnames, rows = db.export_rows(table_name)
    return csv_download_response(f"{table_name}.csv", fieldnames, rows)


def _knowledge_base_confirmation(path: Optional[str]) -> Optional[dict]:
    metadata = get_latest_update()
    selected_path = path or metadata.get("knowledge_base_path")
    if not selected_path:
        return None

    signature = file_signature(selected_path)
    unchanged = (
        metadata.get("knowledge_base_path") == signature["path"]
        and metadata.get("knowledge_base_size") == signature["size"]
        and metadata.get("knowledge_base_modified_at") == signature["modified_at"]
    )
    if not unchanged:
        return None

    return {
        "needs_confirmation": True,
        "reason": "knowledge_base_unchanged",
        "message": (
            "The selected knowledge-base file appears unchanged. Are you sure "
            "you want to continue this update/rebuild anyway?"
        ),
        "metadata": metadata,
        "file": signature,
    }
