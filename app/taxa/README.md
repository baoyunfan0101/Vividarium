# Taxa

Imports the plant knowledge-base workbook and stores the taxonomy tree as an adjacency list.

## Files

- `parser.py`: reads only the workbook sheet named `plants`.
- `db.py`: SQLite schema and taxon operations.
- `service.py`: update/rebuild/query/export API.

## Public API

Import from `app.taxa`.

- `update_taxa(knowledge_base_path=None, db_path=DEFAULT_DB_PATH, max_rows=None, progress=None)`: import workbook rows while preserving existing `taxon_id` values for matching `binomial_name`.
- `rebuild_taxa(knowledge_base_path=None, db_path=DEFAULT_DB_PATH, max_rows=None, progress=None)`: clear `taxa`, reset ids, and import workbook rows in order.
- `get_taxon_by_id(taxon_id, db_path=DEFAULT_DB_PATH)`: return one taxon row.
- `get_taxon_by_binomial(binomial_name, db_path=DEFAULT_DB_PATH)`: return one taxon row by scientific name.
- `get_latest_update(db_path=DEFAULT_DB_PATH)`: return `taxa_metadata` plus `taxa_count`.
- `save_knowledge_base_path(knowledge_base_path, db_path=DEFAULT_DB_PATH)`: save path metadata without importing rows.
- `export_table_csv(table_name, output_path, db_path=DEFAULT_DB_PATH)`: export `taxa` or `taxa_metadata`.

## Workbook Columns

The knowledge base is an Excel workbook. The importer reads only the sheet named `plants`; other sheets are ignored.

The `plants` sheet should contain one row per taxonomy record or species record. Only these columns are read:

| Workbook column | Stored rank/field |
| --- | --- |
| `目(Ordo)` | `ordo` |
| `科(Familia)` | `familia` |
| `属(Genus)` | `genus` |
| `种(Species)` | `species` |
| `学名(Binomial name)` | `binomial_name` |

Header matching tolerates extra text after the leading Chinese label. The rank columns provide the Chinese/common names; `学名(Binomial name)` provides the scientific name used to match photos.

## Tables

### `taxa_metadata`

| Column | Meaning |
| --- | --- |
| `knowledge_base_path` | external workbook path |
| `knowledge_base_size` | workbook size at import/update time |
| `knowledge_base_modified_at` | workbook modified time at import/update time |
| `last_synced_at` | last taxa update/rebuild time |

### `taxa`

| Column | Meaning |
| --- | --- |
| `taxon_id` | autoincrement id |
| `rank` | `ordo`, `familia`, `genus`, or `species` |
| `name` | Chinese/common name from the workbook |
| `parent_id` | adjacency-list parent |
| `binomial_name` | scientific name |

## Tree Rules

Rows are imported in workbook order. Parent links use the nearest active parent rank:

- family -> current order
- genus -> current family, otherwise current order
- species -> current genus, otherwise family, otherwise order

Skipped ranks are therefore valid. A genus can attach directly to an order when no family row is present.
