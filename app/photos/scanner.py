"""Scan photo files and extract metadata."""

from __future__ import annotations

import json
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Iterator


IMAGE_EXTENSIONS = {
    ".arw",
    ".bmp",
    ".cr2",
    ".cr3",
    ".dng",
    ".gif",
    ".heic",
    ".jpeg",
    ".jpg",
    ".nef",
    ".png",
    ".raf",
    ".rw2",
    ".tif",
    ".tiff",
    ".webp",
}


@dataclass(frozen=True)
class FileMetadata:
    width: int | None = None
    height: int | None = None
    captured_at: str | None = None
    camera: str | None = None
    longitude: float | None = None
    latitude: float | None = None
    exif_json: str | None = None


def iter_image_files(root: str | Path) -> Iterator[Path]:
    root_path = Path(root).expanduser().resolve()
    for path in sorted(root_path.rglob("*")):
        if path.is_file() and path.suffix.lower() in IMAGE_EXTENSIONS:
            yield path


def read_file_metadata(path: str | Path) -> FileMetadata:
    try:
        from PIL import ExifTags, Image
    except ImportError:
        return FileMetadata()

    image_path = Path(path)
    try:
        with Image.open(image_path) as image:
            width, height = image.size
            exif = image.getexif()
            exif_dict = _serialize_exif(exif, ExifTags.TAGS)
            latitude, longitude = _extract_gps(exif, ExifTags.GPSTAGS)
            return FileMetadata(
                width=width,
                height=height,
                captured_at=_extract_exif_datetime(exif_dict),
                camera=_extract_exif_camera(exif_dict),
                longitude=longitude,
                latitude=latitude,
                exif_json=(
                    json.dumps(exif_dict, ensure_ascii=False) if exif_dict else None
                ),
            )
    except Exception:
        return FileMetadata()


def _serialize_exif(exif: object, tag_names: dict[int, str]) -> dict[str, str]:
    result: dict[str, str] = {}
    items = getattr(exif, "items", None)
    if items is None:
        return result

    for tag, value in items():
        name = tag_names.get(tag, str(tag))
        if isinstance(value, bytes):
            value = value.decode(errors="ignore")
        result[name] = str(value)
    return result


def _extract_exif_datetime(exif: dict[str, str]) -> str | None:
    for key in ("DateTimeOriginal", "DateTimeDigitized", "DateTime"):
        value = exif.get(key)
        if not value:
            continue
        parsed = _parse_exif_datetime(value)
        if parsed:
            return parsed
    return None


def _extract_exif_camera(exif: dict[str, str]) -> str | None:
    model = exif.get("Model")
    make = exif.get("Make")
    if make and model and make not in model:
        return f"{make} {model}".strip()
    return model or make


def _extract_gps(
    exif: object,
    gps_tag_names: dict[int, str],
) -> tuple[float | None, float | None]:
    gps_tag = 34853
    try:
        gps_info = exif.get_ifd(gps_tag)
    except Exception:
        return None, None
    if not gps_info:
        return None, None

    gps = {gps_tag_names.get(tag, str(tag)): value for tag, value in gps_info.items()}
    latitude = _gps_coordinate(gps.get("GPSLatitude"), gps.get("GPSLatitudeRef"))
    longitude = _gps_coordinate(gps.get("GPSLongitude"), gps.get("GPSLongitudeRef"))
    return latitude, longitude


def _gps_coordinate(value: object, ref: object) -> float | None:
    if not value or not ref:
        return None
    try:
        degrees, minutes, seconds = value
        decimal = float(degrees) + float(minutes) / 60 + float(seconds) / 3600
    except Exception:
        return None
    if str(ref).upper() in {"S", "W"}:
        decimal *= -1
    return decimal


def _parse_exif_datetime(value: str) -> str | None:
    for fmt in ("%Y:%m:%d %H:%M:%S", "%Y-%m-%d %H:%M:%S"):
        try:
            return datetime.strptime(value, fmt).isoformat(sep=" ")
        except ValueError:
            continue
    return None
