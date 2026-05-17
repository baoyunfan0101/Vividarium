"""Small local entry point for inspecting PhytoIndex data."""

from __future__ import annotations

from pathlib import Path

from app.photos.db import DEFAULT_DB_PATH
from app.taxa import export_table_csv


TAXA_EXPORT_PATH = Path("data/taxa_export.csv")


def export_taxa(
    output_path: str | Path = TAXA_EXPORT_PATH,
    db_path: str | Path = DEFAULT_DB_PATH,
) -> int:
    """Read taxa from the database and write them to a CSV file."""

    return export_table_csv("taxa", output_path, db_path=db_path)


def main() -> None:
    exported = export_taxa()
    print(f"Exported {exported} taxa rows to {TAXA_EXPORT_PATH}")


if __name__ == "__main__":
    main()
