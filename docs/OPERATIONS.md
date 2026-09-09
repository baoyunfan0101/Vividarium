# Operation and Audit Backend API

`vividarium_core::operations` defines the DTOs shared by photo rename and
taxonomy history. Domain functions are exported by
`vividarium_core::photos` and `vividarium_core::taxonomy`.

## `OperationSummary`

| Field | Type | Description |
| --- | --- | --- |
| `operation_id` | `i64` | Domain-local operation identity. |
| `kind` | `String` | Operation kind. |
| `source` | `String` | User workflow that created the operation. |
| `applied_at` | `String` | Apply timestamp. |
| `total_items` | `usize` | Total audit units. |
| `succeeded_items` | `usize` | Successful units. |
| `failed_items` | `usize` | Failed units. |
| `rollbackable` | `bool` | Whether rollback is currently supported. |
| `has_formatted_input` | `bool` | Whether taxonomy formatted input is available. |

History list interfaces return summaries only. They never include complete
audit rows or operation source input.

## `OperationAuditRow`

| Field | Type | Description |
| --- | --- | --- |
| `operation_id` | `i64` | Parent operation. |
| `sequence` | `usize` | One-based order within the operation. |
| `entity_type` | `String` | Changed entity category. |
| `entity_id` | `Option<String>` | Stable entity identity when available. |
| `action` | `String` | Performed or attempted action. |
| `before_json` | `Option<Value>` | Relevant state before the action. |
| `after_json` | `Option<Value>` | Relevant state after the action. |
| `succeeded` | `bool` | Whether this audit row succeeded. |
| `message` | `String` | Result or failure explanation. |

Photo rename rows use `entity_type = "photo"` and `action = "rename"`.
Their JSON state contains `directory_relative_path` and `filename`.
Taxonomy rows use the same fields for taxon and taxon-name changes.

## `OperationInput`

Operation input records what the user submitted, independently from audit rows
that record what changed. The source-aware variants are:

| Kind | Content |
| --- | --- |
| `custom_sql` | Exact submitted SQL, including comments and whitespace. |
| `formatted_update` | Ordered `TaxonInputRow` values from the replayable formatted input store. |
| `taxonomy_action` | Direct taxonomy action name and submitted request fields. |

`get_operation_input` loads this detail separately from summary and audit
pagination. Operations created without source input return `None`.

## `OperationPage<T>`

| Field | Type | Description |
| --- | --- | --- |
| `items` | `Vec<T>` | Current page. |
| `next_cursor` | `Option<String>` | Opaque next-page cursor. |

Pass `None` for the first page. A cursor is scoped to its interface,
operation, and domain. Limits are clamped to `1..=500`.

## Domain interfaces

Both `photos` and `taxonomy` expose:

| Function | Parameters after `database` | Return |
| --- | --- | --- |
| `list_operations` | `cursor: Option<&str>`, `limit: usize` | `OperationPage<OperationSummary>` |
| `list_operation_audit` | `operation_id: i64`, `cursor: Option<&str>`, `limit: usize` | `OperationPage<OperationAuditRow>` |
| `write_operation_audit` | `operation_id: i64`, `writer: &mut W` where `W: Write` | `()` |
| `write_operations_audit` | `operation_ids: &[i64]`, `writer: &mut W` where `W: Write` | `()` |
| `write_all_operation_audit` | `writer: &mut W` where `W: Write` | `()` |
| `rollback_operation` | `operation_id: i64` | `()` |

Audit CSV columns are identical in both domains:

```text
operation_id,sequence,entity_type,entity_id,action,before_json,after_json,succeeded,message
```

Rows stream directly from the database to the caller-provided writer in
operation and sequence order. Batch and all-history exports use one header;
the complete CSV is never materialized by the export interface. The delimiter
is comma by default and follows the application-wide General setting; supported
values are comma, semicolon, tab, and pipe.

Taxonomy rollback applies the inverse changeset in one transaction while
preserving relational dependencies. It then verifies foreign-key integrity,
validates the taxonomy, records pending photo-library synchronization, and
deletes the original operation. A data, row, constraint, or foreign-key
conflict aborts the transaction with a diagnostic rollback error, preserving
the taxonomy and every operation-owned record. Successful rollback removes the
summary, audit rows, changeset, source input, and formatted input. No
user-visible reverse operation is created.

## Taxonomy operation input

Formatted updates set `has_formatted_input` and retain their submitted rows as
canonical replayable input. Custom SQL retains its exact script once at the
operation level. Direct taxonomy actions retain their submitted action input.

The taxonomy module additionally exposes:

| Function | Parameters after `database` | Return |
| --- | --- | --- |
| `get_operation_input` | `operation_id: i64` | `Option<OperationInput>` |
| `export_operation_input` | `operation_id: i64` | Formatted-update CSV `String` |
| `export_operations_input` | `operation_ids: &[i64]` | Formatted-update CSV `String` |
| `export_all_replayable_inputs` | none | Formatted-update CSV `String` |

Delete and custom SQL operations are not formatted-input CSV operations.
Selected input export returns an error if any selected operation is not
replayable. Desktop audit and formatted-input export commands take an absolute
destination path selected by the caller and write the resulting CSV there with
the same configured delimiter.
