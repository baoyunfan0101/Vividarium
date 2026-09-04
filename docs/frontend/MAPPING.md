# Mapping Domain

Location: `apps/desktop/src/features/mapping`

The Mapping domain displays persistent photo-to-taxon state and provides the
editor used by both the Mapping workspace and single-photo tabs.

## Public interfaces

### `MappingView(props)`

Parameters include `PhotoOpenHandlers` and an optional active flag.

Returns: the singleton mapping workspace with state tabs, cursor-backed photo
results, filename or taxon search for Matched items, photo preview, and an
embedded editor.

### `MappingEditor(props)`

Parameters:

- `photo`: the photo being edited.
- `embedded`: whether the editor is placed inside the Mapping workspace.
- `active`: whether the editor is currently active.
- `onPhotoTaxonDisplayState`: receives the current photo taxonomy display state.
- `handlers`: shared `PhotoOpenHandlers`; the editor uses its taxon-opening
  handler for Current mapping, ambiguous candidates, and taxonomy search results.
- `onStatus`: reports current-photo action feedback through the owning tab.
- `refreshKey`: optional external invalidation value.

Returns: current mapping details, persisted Ambiguous candidates, taxonomy
search, and controls to clear, assign, replace, or automatically remap. Current
mapping, ambiguous candidates, and search results use the same taxon-card
interaction: selecting the card opens Taxon Detail, while Map, Select, and
Unmap perform only their mapping action. Non-primary names that produced a
match are shown beneath accepted names with the shared taxonomy-search
explanation. Mapping taxon cards use the same match explanation as Taxonomy
Search. Automatic matches and ambiguous candidates identify matched scientific
synonyms, Chinese aliases, and English aliases when that provenance is
available. Manual taxon assignment does not invent matched-name provenance.
Taxon-card text is selectable and copyable without triggering navigation.

The Mapping workspace exposes independent dividers between the photo list,
preview, and editor. The standalone editor also separates its photo from the
controls, while both standalone and embedded editors allow the current match
and taxonomy-search sections to be resized vertically.

### `MappingBadge({ status })`

Parameters: `PhotoTaxonStatus`.

Returns: a compact, color-coded status label.

## Status use

The UI treats `matched`, `ambiguous`, and `unmatched` as long-lived states and
`processing` as the visible state while a photo is queued. Navigation to a
taxon is enabled only when the lightweight mapping response contains Matched
state and a taxon ID.

Photo mapping status is reused by the current-photo status bar and the photo
context menu. `View taxon details` is enabled only when the photo has `matched`
state with a taxon ID.
