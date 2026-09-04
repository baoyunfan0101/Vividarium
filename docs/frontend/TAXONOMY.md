# Taxonomy Domain

Location: `apps/desktop/src/features/taxonomy`

The Taxonomy domain owns taxon browsing and taxonomy mutation workspaces. It
can operate without an available Photo Library except for explicit requests
that list photos belonging to a taxon.

## Public pages

### `TaxonomySearchView(props)`

Parameters include an optional selected taxon, a callback for the currently
displayed taxon, and a callback for opening taxon photos.

Returns: taxon search results or a single-taxon page with breadcrumb and child
navigation. Typing requests lightweight suggestions after a 260 ms input
pause. Arrow keys select a suggestion, and Enter or a pointer selection
submits the full search. A pointer action outside the search closes
suggestions. Suggestion rows show available scientific, Chinese, and English
names followed by rank. Submitted searches show a centered searching status
until results resolve. Every formal submission starts a fresh request even
when the normalized query is unchanged, and the result list returns to its
first row for the new response.
Taxonomy Search reports searching state and the number of results currently
shown in the status bar.

Hierarchy breadcrumbs, the current heading, and child navigation use the
Taxonomy visible-name preference for scientific, Chinese, and English accepted
names. The complete name-group editor always shows the stored records and is
independent of this display preference.

Every taxon requires one scientific accepted name, which cannot be deleted.
Chinese and English accepted names are optional and unique. A localized
accepted name can be deleted only while its corresponding alias group is
empty. Chinese and English aliases require their corresponding accepted name.
Synonyms and localized aliases remain independently deletable and promotable.

### `FormattedUpdateView({ onStatus, mutationDisabled })`

Parameters: a status callback and optional mutation guard.

Returns: CSV upload and template actions, an editable formatted-input table,
preview, apply, and row-level result logs. Preview stores a backend candidate
and enables Apply. Apply consumes that candidate and directly commits its
precomputed changeset. Editing rows, importing another file, changing the name
separator or CSV delimiter, or receiving another taxonomy mutation clears the
preview and disables Apply until Preview succeeds again. The current taxonomy
name separator is loaded from Settings metadata. CSV uploads and downloaded
templates use the application-wide CSV delimiter. The leading Help action
opens a compact summary of species-to-genus normalization, lowest-rank
matching, ancestor disambiguation, and strict-parent recursive creation.

### `CustomSqlView(props)`

Parameters include a status callback and optional mutation guard.

Returns: a SQL editor, execution messages, typed result sets, warnings, and
complete CSV export for every read-only query result. Multi-statement output is
grouped by executable statement index. Result tables share an execution-wide
minimum width determined by the largest result column count; each statement
still displays only its real columns. Long values remain single-line and use
ellipsis without changing row-specific column widths. The latest successful
execution and its SQL remain visible while the editor changes or a later Run
fails, and every Export action uses that executed SQL snapshot and its result
statement index. The script-level execution summary is reported through the tab
status bar. The output pane scrolls across statement sections that exceed the
available height, while each result table scrolls its own rows. A statement
that both mutates and returns rows uses one combined result header. Its source
sidebar has two mutually exclusive VS Code-style
groups: Input sources is expanded by default, and All accessible tables shows
only the complete readable internal `main` taxonomy schema. Uploaded sources
remain exclusively in Input sources. The expanded group body scrolls
independently. CSV sources and exports use the application-wide CSV delimiter.
Running operations show the current phase, the active statement index during
SQL execution, and elapsed time. Long-running statements have an execution
limit, and a timeout appears as an operation failure. A successful execution
snapshot remains the source for result display and export.
The leading Help action opens the integer mappings for `taxa.rank` and
`taxon_names.name_type`.

### `SqlImportSettings({ onApplied })`

Parameters: optional callback invoked after an SQL Import is applied.

Returns: the SQL Import source registry, SQL workspace, validation issues,
metadata, and apply controls. Its source sidebar uses the same mutually
exclusive groups and scrolling behavior as Custom SQL. All accessible tables
shows only the current internal taxonomy schema, which SQL Import can read
through the `taxonomy` alias. Uploaded sources and the `sql_import` staging
schema appear only in Input sources, so neither is duplicated in All
accessible tables. Its leading Help action shows the same taxonomy integer
code mappings as Custom SQL. SQL Import operations show the current phase, the
active statement index, and elapsed time. Staging and validation phases appear
only after SQL execution succeeds.

### `DirectImportSettings({ onApplied })`

Parameters: optional callback invoked after a Direct Import is applied.

Returns: a native SQLite selection action, a staged source card containing the
validated path, tables, and columns, an explicit confirmation action,
background import status, and the committed replacement result or validation
error. Selecting a database only inspects it; replacement begins only after
confirmation.

Search results and taxon details, formatted input and result logs, SQL input
sources and editors, and execution or validation outputs use adjustable panes
with minimum sizes appropriate to their controls.

`SqlInputList` owns the shared SQL source sidebar. Only one top-level group is
expanded at a time; selecting the expanded group collapses it. Table rows may
still be expanded independently to inspect their ordered columns and declared
types.

## Reusable interfaces

`TaxonCard` renders a `TaxonSummary` with optional selection and actions. Its
third line displays available Chinese and English names separated by a middle
dot, or a dash when both names are absent. Taxon cards always retain this full
accepted-name presentation and do not use visible-name preferences.
Taxon-card text is selectable and copyable. Dragging a text selection does not
run card navigation, and action buttons remain isolated from the card action.
Taxonomy search cards display accepted taxon names as their primary identity.
When a result also matches through a scientific synonym, Chinese alias, or
English alias, the card shows each matching non-accepted name explicitly.
Accepted-name matches do not suppress these explanations.
`useTaxonSuggestions(query, enabled)` returns lightweight suggestions after a
260 ms input pause; only the latest request may publish its low-priority
result. `useTaxonSearch(query, options)` returns submitted search results,
loading state, and an error message. `emitTaxonomyMutation` and
`useTaxonomyMutation` coordinate refresh after committed taxonomy changes.
