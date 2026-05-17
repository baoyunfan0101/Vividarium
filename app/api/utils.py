"""Shared API helpers."""

from __future__ import annotations

import csv
from io import StringIO
from datetime import datetime
from pathlib import Path
from typing import Any, Optional

from fastapi.responses import Response


def file_signature(path: str | Path) -> dict[str, Any]:
    resolved = Path(path).expanduser()
    stat = resolved.stat()
    return {
        "path": str(resolved),
        "size": stat.st_size,
        "modified_at": datetime.fromtimestamp(stat.st_mtime).isoformat(
            sep=" ",
            timespec="seconds",
        ),
    }


def iso_after(left: Optional[str], right: Optional[str]) -> bool:
    if not left or not right:
        return False
    return left > right


def csv_download_response(
    filename: str,
    fieldnames: list[str],
    rows: list[dict],
) -> Response:
    buffer = StringIO()
    writer = csv.DictWriter(buffer, fieldnames=fieldnames)
    writer.writeheader()
    writer.writerows(rows)
    content = "\ufeff" + buffer.getvalue()
    return Response(
        content=content,
        media_type="text/csv; charset=utf-8",
        headers={"Content-Disposition": f'attachment; filename="{filename}"'},
    )
