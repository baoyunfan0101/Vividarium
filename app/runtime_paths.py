"""Runtime paths shared by development and packaged builds."""

from __future__ import annotations

import os
import sys
from pathlib import Path


APP_NAME = "PhytoIndex"
PROJECT_ROOT = Path(__file__).resolve().parent.parent


def is_frozen() -> bool:
    return bool(getattr(sys, "frozen", False))


def data_dir() -> Path:
    configured = os.environ.get("PHYTOINDEX_DATA_DIR")
    if configured:
        return Path(configured).expanduser().resolve()
    if is_frozen():
        return _user_data_dir()
    return PROJECT_ROOT / "data"


def database_path() -> Path:
    return data_dir() / "phytoindex.db"


def thumbnail_dir() -> Path:
    return data_dir() / "thumbnails"


def frontend_dist_dir() -> Path:
    configured = os.environ.get("PHYTOINDEX_FRONTEND_DIST")
    if configured:
        return Path(configured).expanduser().resolve()
    if is_frozen() and hasattr(sys, "_MEIPASS"):
        return Path(sys._MEIPASS) / "frontend" / "dist"  # type: ignore[attr-defined]
    return PROJECT_ROOT / "frontend" / "dist"


def _user_data_dir() -> Path:
    if sys.platform.startswith("win"):
        base = os.environ.get("APPDATA")
        if base:
            return Path(base) / APP_NAME
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / APP_NAME
    return Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local" / "share")) / APP_NAME
