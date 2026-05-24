"""Runtime operation status API."""

from __future__ import annotations

from fastapi import APIRouter

from app.operations import operation_manager


router = APIRouter(prefix="/operations", tags=["operations"])


@router.get("/status")
def api_operation_status() -> dict:
    return {"operations": operation_manager.status()}
