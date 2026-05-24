# Photos-Taxa Mapping

Maps `photos.photo_id` to `taxa.taxon_id` and stores a cached taxonomy subtree for taxa that appear in photos.

## Files

- `db.py`: mapping tables and subtree queries.
- `service.py`: update/rebuild/query/export API.

## Public API

Import from `app.photos_taxa_mapping`.

- `update_mapping(db_path=DEFAULT_DB_PATH, progress=None)`: map photos whose status is `new` or `updated`; also removes orphan mappings.
- `rebuild_mapping(db_path=DEFAULT_DB_PATH, progress=None)`: clear mapping tables and map all photos.
- `get_by_taxon_id(taxon_id, db_path=DEFAULT_DB_PATH)`: return `{taxon, photo_ids, children}`. `taxon_id=None` returns root order nodes.
- `get_by_binomial_name(binomial_name, db_path=DEFAULT_DB_PATH)`: find a cached taxon by scientific name and return the same shape.
- `get_by_name(name, db_path=DEFAULT_DB_PATH)`: find a cached taxon by Chinese/common name and return the same shape.
- `suggest_taxa(query, mode, limit=10, db_path=DEFAULT_DB_PATH)`: suggest cached taxa by `name` or `binomial_name`.
- `get_latest_update(db_path=DEFAULT_DB_PATH)`: return metadata plus mapped photo and subtree row counts.
- `export_table_csv(table_name, output_path, db_path=DEFAULT_DB_PATH)`: export a mapping table.

## Mapping Algorithm

For each selected photo:

1. Read `photos.binomial_name`.
2. If it exists in `photos_taxa_mapping_taxa`, use that taxon.
3. Otherwise search `taxa` by `binomial_name`.
4. If found, copy the taxon and all ancestors into `photos_taxa_mapping_taxa`, preserving `taxon_id`.
5. If not found, map the photo to `SPECIAL_UNMAPPED_TAXON_ID = 0` and return it in `unmapped_photos`.

## Tables

### `photos_taxa_mapping_metadata`

| Column | Meaning |
| --- | --- |
| `last_synced_at` | mapping update/rebuild time |
| `photos_last_synced_at` | photos sync time observed during mapping |
| `taxa_last_synced_at` | taxa sync time observed during mapping |

### `photos_taxa_mapping`

| Column | Meaning |
| --- | --- |
| `photo_id` | primary key from `photos` |
| `taxon_id` | taxon id from `taxa`, or `0` when unmapped |

### `photos_taxa_mapping_taxa`

Cached subtree with the same fields as `taxa`:

| Column |
| --- |
| `taxon_id` |
| `rank` |
| `name` |
| `parent_id` |
| `binomial_name` |

Only taxa needed by mapped photos, plus their ancestors, are stored.
