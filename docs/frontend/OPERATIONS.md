# Operations Domain

Location: `apps/desktop/src/features/operations`

The Operations domain presents audit history for photo rename and taxonomy
mutation operations.

## Public interface

### `OperationHistoryView(props)`

Parameters:

- `domain`: `"photo"` or `"taxonomy"`.
- `onStatus`: callback receiving user-facing completion or error text.

Returns: cursor-backed operation summaries and audit rows.

Operation summary rows are selectable content surfaces. Summary text can be
copied while clicking the row still opens the operation detail; selection
checkboxes and operation actions remain independent controls.

Both history lists use the full available width, keep each operation summary on
one row, and support selecting individual loaded operations or all loaded
operations. Audit export and rollback apply only to the selection. Taxonomy
history also exports the replayable formatted inputs contained in the current
selection, combining them into one CSV and ignoring selected operations that
do not have formatted input. The action is disabled only when the selection has
no replayable operation. Export actions always open the native CSV destination
dialog. Photo and taxonomy audit exports plus taxonomy replayable-input exports
use the application-wide CSV delimiter selected in General settings.

Opening an operation replaces the list with a source-aware detail containing
separate Input and Changes sections. Custom SQL input uses a selectable,
read-only SQL editor. Formatted Update input uses a horizontally scrollable
table in the same logical column order as the update workflow. Direct taxonomy
actions show their action and submitted fields. Historical operations without
stored input show a neutral unavailable message.

Changes remain backed by paginated audit rows. Applicable audit before and
after values use syntax-highlighted, indented JSON in content-sized editors;
Custom SQL changes prioritize affected entity rows instead of technical
changeset-size JSON. The detail toolbar has a back button and actions scoped to
the operation. A successful rollback removes the operation and refreshes the
appropriate domain through its mutation notification. Diagnostic backend
rollback errors are displayed directly. Batch rollback runs selected
operations from newest to oldest.
