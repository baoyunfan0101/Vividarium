"""FastAPI entry point for PhytoIndex."""

from __future__ import annotations

import os

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles

from app.runtime_paths import frontend_dist_dir, is_frozen
from .local_paths import router as local_paths_router
from .map import router as map_router
from .operations import router as operations_router
from .photos import router as photos_router
from .photos_taxa_mapping import router as mapping_router
from .taxa import router as taxa_router


def create_app() -> FastAPI:
    app = FastAPI(title="PhytoIndex API")
    app.add_middleware(
        CORSMiddleware,
        allow_origins=[
            "http://127.0.0.1:5173",
            "http://localhost:5173",
        ],
        allow_credentials=True,
        allow_methods=["*"],
        allow_headers=["*"],
    )
    app.include_router(local_paths_router, prefix="/api")
    app.include_router(operations_router, prefix="/api")
    app.include_router(photos_router, prefix="/api")
    app.include_router(taxa_router, prefix="/api")
    app.include_router(mapping_router, prefix="/api")
    app.include_router(map_router, prefix="/api")

    if _should_serve_frontend():
        dist = frontend_dist_dir()
        if dist.exists():
            app.mount("/", StaticFiles(directory=dist, html=True), name="frontend")
    return app


def _should_serve_frontend() -> bool:
    return is_frozen() or os.environ.get("PHYTOINDEX_SERVE_FRONTEND") == "1"


app = create_app()
