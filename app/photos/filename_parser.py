"""Optional filename metadata parser for photo imports.

Recognized heuristic format:
    <binomial_name><YYYYMMDD>_<HHMMSS><location><device>.<ext>

Photos are still indexed when filenames do not match this pattern.

Example:
    Lilium brownii20260504_123525GardenA iPhone12.jpg
    Lilium brownii 'Viridulum'20260504_123525GardenA iPhone12.jpg
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path


FILENAME_PATTERN = re.compile(
    r"^(?P<binomial_name>.+?)(?P<date>\d{8})_(?P<time>\d{6})(?P<trailing>.*)$"
)
DEVICE_PATTERN = re.compile(
    r"(?P<device>(?:iPhone|iPad|Pixel|Canon|Nikon|Sony|FUJIFILM|Fujifilm|HUAWEI|Huawei|Xiaomi|OPPO|vivo|Vivo)[A-Za-z0-9 _.-]*)$"
)


@dataclass(frozen=True)
class ParsedFilename:
    binomial_name: str | None = None
    shoot_date: str | None = None
    location: str | None = None
    device: str | None = None


def parse_photo_filename(filename: str) -> ParsedFilename:
    """Parse filename metadata when it matches the heuristic format."""

    stem = Path(filename).stem
    match = FILENAME_PATTERN.match(stem)
    if not match:
        return ParsedFilename()

    location, device = _split_location_device(match.group("trailing").strip())
    return ParsedFilename(
        binomial_name=match.group("binomial_name").strip() or None,
        shoot_date=_parse_filename_datetime(match.group("date"), match.group("time")),
        location=location,
        device=device,
    )


def _parse_filename_datetime(date_text: str, time_text: str) -> str | None:
    try:
        return datetime.strptime(
            f"{date_text}_{time_text}",
            "%Y%m%d_%H%M%S",
        ).isoformat(sep=" ")
    except ValueError:
        return None


def _split_location_device(text: str) -> tuple[str | None, str | None]:
    if not text:
        return None, None

    match = DEVICE_PATTERN.search(text)
    if not match:
        return text, None

    device = match.group("device").strip()
    location = text[: match.start("device")].strip() or None
    return location, device
