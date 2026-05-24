"""Map API routes."""

from __future__ import annotations

from typing import Optional

from fastapi import APIRouter, HTTPException, Query

from app.map import list_map_photos


router = APIRouter(prefix="/map", tags=["map"])


@router.get("/photos")
def api_list_map_photos(bbox: Optional[str] = Query(None)) -> dict:
    return {"photos": list_map_photos(bbox=_parse_bbox(bbox))}


def _parse_bbox(value: Optional[str]) -> Optional[tuple[float, float, float, float]]:
    if not value:
        return None
    try:
        parts = tuple(float(part) for part in value.split(","))
    except ValueError as exc:
        raise HTTPException(status_code=400, detail="bbox must contain numeric values") from exc
    if len(parts) != 4:
        raise HTTPException(status_code=400, detail="bbox must be minLng,minLat,maxLng,maxLat")
    min_lng, min_lat, max_lng, max_lat = parts
    if min_lng > max_lng or min_lat > max_lat:
        raise HTTPException(status_code=400, detail="bbox min values must be <= max values")
    return min_lng, min_lat, max_lng, max_lat
