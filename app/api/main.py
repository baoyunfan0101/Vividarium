"""FastAPI entry point for PhytoIndex."""

from __future__ import annotations

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

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
    app.include_router(photos_router)
    app.include_router(taxa_router)
    app.include_router(mapping_router)
    return app


app = create_app()
