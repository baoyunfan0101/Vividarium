"""Photos API routes."""

from __future__ import annotations

from pathlib import Path
from typing import List, Optional

from fastapi import APIRouter, HTTPException
from fastapi.responses import FileResponse
from pydantic import BaseModel

from app.photos import (
    export_table_csv,
    get_latest_update,
    get_photo,
    get_roots,
    list_changed_photos,
    list_directory,
    list_photos,
    rebuild_photos,
    save_roots,
    update_photos,
)
from app.photos.db import PhotosDatabase

from .utils import csv_download_response

router = APIRouter(prefix="/photos", tags=["photos"])


class PhotosRebuildRequest(BaseModel):
    roots: Optional[List[str]] = None


class PhotosUpdateRequest(BaseModel):
    root: Optional[str] = None
    roots: Optional[List[str]] = None


class PhotosRootsRequest(BaseModel):
    roots: List[str]


class ExportRequest(BaseModel):
    table_name: str
    output_path: str


@router.get("/roots")
def api_get_roots() -> dict:
    return {"roots": get_roots(), "metadata": get_latest_update()}


@router.put("/roots")
def api_save_roots(request: PhotosRootsRequest) -> dict:
    return {"metadata": save_roots([Path(root) for root in request.roots])}


@router.get("/browse")
def api_browse_photos(
    root: str,
    relative_dir: str = "",
) -> dict:
    return list_directory(root, relative_dir)


@router.get("/all")
def api_list_photos() -> dict:
    return {"photos": list_photos()}


@router.get("/changed")
def api_list_changed_photos() -> dict:
    return {"photos": list_changed_photos()}


@router.get("/latest-update")
def api_get_latest_update() -> dict:
    return {"metadata": get_latest_update()}


@router.get("/file/{photo_id}")
def api_get_photo_file(photo_id: int) -> FileResponse:
    photo = get_photo(photo_id)
    if photo is None:
        raise HTTPException(status_code=404, detail="photo not found")

    file_path = _photo_file_path(photo)
    if not file_path.exists() or not file_path.is_file():
        raise HTTPException(status_code=404, detail="photo file not found")
    return FileResponse(file_path)


@router.get("/export")
def api_download_photos_table(table_name: str) -> object:
    with PhotosDatabase() as db:
        fieldnames, rows = db.export_rows(table_name)
    return csv_download_response(f"{table_name}.csv", fieldnames, rows)


@router.get("/{photo_id}")
def api_get_photo(photo_id: int) -> dict:
    photo = get_photo(photo_id)
    if photo is None:
        raise HTTPException(status_code=404, detail="photo not found")
    return {"photo": photo}


@router.post("/update")
def api_update_photos(request: PhotosUpdateRequest) -> dict:
    if request.roots:
        results = {
            root: update_photos(root)
            for root in request.roots
        }
        return {"result": {"roots": len(request.roots), "results": results}}

    root = request.root
    if root is None:
        roots = get_roots()
        if len(roots) != 1:
            raise HTTPException(
                status_code=400,
                detail="root is required unless exactly one root is recorded",
            )
        root = roots[0]
    return {"result": update_photos(root)}


@router.post("/rebuild")
def api_rebuild_photos(request: PhotosRebuildRequest) -> dict:
    roots = request.roots or get_roots()
    if not roots:
        raise HTTPException(status_code=400, detail="roots are required")
    return {"result": rebuild_photos([Path(root) for root in roots])}


@router.post("/export")
def api_export_photos(request: ExportRequest) -> dict:
    exported = export_table_csv(request.table_name, request.output_path)
    return {"exported": exported, "output_path": request.output_path}


def _photo_file_path(photo: dict) -> Path:
    root = Path(photo["root"]).expanduser().resolve()
    candidate = (root / photo["relative_path"]).resolve()
    try:
        candidate.relative_to(root)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail="photo path escapes root") from exc
    return candidate
