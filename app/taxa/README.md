# Taxa Module

The `taxa` module imports and queries the plant taxonomy tree stored in the
external plant knowledge-base workbook. It only reads the workbook sheet named
`plants`.

## Files

- `parser.py`: reads the external workbook and yields normalized `TaxaRow`
  objects from the selected columns.
- `db.py`: owns the SQLite schema and low-level database operations.
- `service.py`: exposes the public module API for update, rebuild, query, and
  metadata lookup.
- `__init__.py`: re-exports the public service API.

## Public API

Import from `app.taxa` for normal use:

```python
from app.taxa import (
    update_taxa,
    rebuild_taxa,
    get_taxon_by_id,
    get_taxon_by_binomial,
    get_latest_update,
    export_table_csv,
)
```

### `update_taxa(knowledge_base_path=None, db_path=DEFAULT_DB_PATH, max_rows=None)`

Updates the `taxa` table from the external workbook.

- If `knowledge_base_path` is omitted, the previously recorded workbook path in
  `taxa_metadata` is used.
- Existing rows with the same `binomial_name` keep their original `taxon_id`.
- Existing rows with the same `binomial_name` have `rank`, `name`, and
  `parent_id` updated.
- Workbook taxa with unseen `binomial_name` values are inserted as new rows.
- Returns a summary dict with workbook path, workbook modified time, sheet name,
  number of rows read, number of taxa processed, and total taxa count.

### `rebuild_taxa(knowledge_base_path=None, db_path=DEFAULT_DB_PATH, max_rows=None)`

Rebuilds the `taxa` table from scratch.

- If `knowledge_base_path` is omitted, the previously recorded workbook path in
  `taxa_metadata` is used.
- Existing `taxa` rows are deleted before import.
- `taxon_id` values may change.
- Returns the same summary shape as `update_taxa`.

### `get_taxon_by_id(taxon_id, db_path=DEFAULT_DB_PATH)`

Returns one `taxa` row as a dict, or `None` when the id does not exist.

### `get_taxon_by_binomial(binomial_name, db_path=DEFAULT_DB_PATH)`

Returns the first `taxa` row matching the scientific name in `binomial_name`, or
`None` when it does not exist.

### `get_latest_update(db_path=DEFAULT_DB_PATH)`

Returns the recorded external workbook path, workbook modified time, and latest
taxa import/update time:

```python
{
    "knowledge_base_path": "...",
    "knowledge_base_size": 12345,
    "knowledge_base_modified_at": "YYYY-MM-DD HH:MM:SS",
    "last_synced_at": "YYYY-MM-DD HH:MM:SS",
}
```

### `export_table_csv(table_name, output_path, db_path=DEFAULT_DB_PATH)`

Exports a taxa module table to CSV. Valid table names:

- `taxa`
- `taxa_metadata`

Returns the number of exported data rows.

## Database Tables

### `taxa_metadata`

Stores metadata about the external knowledge base and import state.

| Column | Type | Meaning |
| --- | --- | --- |
| `knowledge_base_path` | `TEXT` | External workbook path. |
| `knowledge_base_size` | `INTEGER` | Filesystem size of the workbook at import/update time. |
| `knowledge_base_modified_at` | `TEXT` | Filesystem modification time of the workbook at import/update time. |
| `last_synced_at` | `TEXT` | Local time when the `taxa` table was last updated or rebuilt. |

### `taxa`

Stores the taxonomy tree as an adjacency list.

| Column | Type | Meaning |
| --- | --- | --- |
| `taxon_id` | `INTEGER PRIMARY KEY AUTOINCREMENT` | Stable row id, except during rebuilds. |
| `rank` | `TEXT NOT NULL` | One of `ordo`, `familia`, `genus`, `species`. |
| `name` | `TEXT NOT NULL` | The Chinese/common taxonomy name from the workbook column. |
| `parent_id` | `INTEGER` | Parent `taxon_id`; this is the adjacency-list link. |
| `binomial_name` | `TEXT` | The scientific name from the workbook's `Binomial name` column. |

Indexes:

- `idx_taxa_parent` on `parent_id`
- `idx_taxa_binomial` on `binomial_name`

## Workbook Parsing

Only the sheet named `plants` is read. Other sheets are ignored.

The parser extracts these columns:

- `目(Ordo)` -> `ordo`
- `科(Familia)` -> `familia`
- `属(Genus)` -> `genus`
- `种(Species)` -> `species`
- `学名(Binomial name)` -> `binomial_name`

Header matching is tolerant of extra characters after the leading Chinese label,
so headers such as `种×(Species)` and `学名×(Binomial name)...` still match.

## Tree-Building Logic

Rows are read in workbook order. The importer tracks the latest active parent at
each rank and creates parent links as an adjacency list.

Skipped ranks are allowed:

- A family attaches to the current order.
- A genus attaches to the nearest current parent among family, then order.
- A species attaches to the nearest current parent among genus, family, then
  order.

For example, if `Peliaina` appears under `蓝载藻目` with no family row between
them, the genus row is linked directly to the order:

```text
蓝载藻目 -> Peliaina
```

## Update vs. Rebuild

`update_taxa` is for normal refreshes. It preserves `taxon_id` when a
`binomial_name` already exists, which protects references from other modules.
It always performs the update when called; callers decide whether to skip based
on the metadata returned by `get_latest_update`.

`rebuild_taxa` is for schema resets or deliberate full reloads. It clears the
table first, so ids are recreated according to workbook order.

The `last_synced_at` value is a module-level update marker. Downstream modules
such as `photos_taxa_mapping` can compare it with `photos.photos_metadata`
`last_synced_at` values to detect stale mappings.
