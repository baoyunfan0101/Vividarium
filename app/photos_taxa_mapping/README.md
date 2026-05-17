# Photos-Taxa Mapping Module

The `photos_taxa_mapping` module links rows from `photos.photos` to rows from
`taxa.taxa`. Table names are prefixed with `photos_taxa_mapping` so future
mapping modules can use their own table groups without collisions.

## Public API

Import from `app.photos_taxa_mapping`:

```python
from app.photos_taxa_mapping import (
    update_mapping,
    rebuild_mapping,
    get_by_taxon_id,
    get_by_binomial_name,
    get_by_name,
    get_latest_update,
    export_table_csv,
)
```

### `update_mapping(db_path=DEFAULT_DB_PATH)`

Processes only photos whose status is `updated` or `new`.

For each photo, the mapper:

1. Looks for `photos.binomial_name` in `photos_taxa_mapping_taxa`.
2. If found, upserts the row in `photos_taxa_mapping`.
3. If not found, looks for the same binomial name in `taxa`.
4. If found, copies that taxon and all ancestors into
   `photos_taxa_mapping_taxa`, preserving `taxon_id`, then upserts the mapping.
5. If not found, maps the photo to `SPECIAL_UNMAPPED_TAXON_ID = 0` and returns
   the photo in `unmapped_photos`.

### `rebuild_mapping(db_path=DEFAULT_DB_PATH)`

Clears both `photos_taxa_mapping` and `photos_taxa_mapping_taxa`, then processes
all rows from `photos`.

The mapping algorithm is identical to `update_mapping`.

### `get_by_taxon_id(taxon_id, db_path=DEFAULT_DB_PATH)`

Returns:

```python
{
    "taxon": {...} | None,
    "photo_ids": [...],
    "children": [...],
}
```

When `taxon_id` is `None`, `photo_ids` is empty and `children` contains all root
order nodes in the subtree table.

### `get_by_binomial_name(binomial_name, db_path=DEFAULT_DB_PATH)`

Finds a taxon by `binomial_name` in `photos_taxa_mapping_taxa`, then returns the
same shape as `get_by_taxon_id`.

### `get_by_name(name, db_path=DEFAULT_DB_PATH)`

Finds all taxa with the given Chinese/common `name` in
`photos_taxa_mapping_taxa`, then returns the same shape as `get_by_taxon_id`
for the first matching row.

### `get_latest_update(db_path=DEFAULT_DB_PATH)`

Returns mapping metadata:

```python
{
    "last_synced_at": "...",
    "photos_last_synced_at": "...",
    "taxa_last_synced_at": "...",
}
```

### `export_table_csv(table_name, output_path, db_path=DEFAULT_DB_PATH)`

Exports one mapping table to CSV. Valid table names:

- `photos_taxa_mapping_metadata`
- `photos_taxa_mapping`
- `photos_taxa_mapping_taxa`

Returns the number of exported data rows.

## Database Tables

### `photos_taxa_mapping_metadata`

| Column | Type | Meaning |
| --- | --- | --- |
| `last_synced_at` | `TEXT` | Time when this mapping module last synced. |
| `photos_last_synced_at` | `TEXT` | Latest photos module sync time observed during mapping. |
| `taxa_last_synced_at` | `TEXT` | Taxa module sync time observed during mapping. |

### `photos_taxa_mapping`

| Column | Type | Meaning |
| --- | --- | --- |
| `photo_id` | `INTEGER PRIMARY KEY` | Photo id from `photos.photos`. |
| `taxon_id` | `INTEGER NOT NULL` | Taxon id from `taxa.taxa`, or `0` for unmapped photos. |

### `photos_taxa_mapping_taxa`

This is a subtree cache of `taxa.taxa`. It has the same columns:

| Column | Type |
| --- | --- |
| `taxon_id` | `INTEGER PRIMARY KEY` |
| `rank` | `TEXT NOT NULL` |
| `name` | `TEXT NOT NULL` |
| `parent_id` | `INTEGER` |
| `binomial_name` | `TEXT` |

Only taxa needed by mapped photos, plus their ancestors, are stored here.
