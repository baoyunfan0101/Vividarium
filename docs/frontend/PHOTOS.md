# Photos Domain

Location: `apps/desktop/src/features/photos`

The Photos domain owns photo browsing and interactions. Folder, photographed
taxonomy, map, search Photo Sets, detail views, media rendering, and the photo
context menu share the same selection and mutation behavior.

Folder, photographed-taxon, and photo list rows are selectable content surfaces:
their displayed names can be copied while normal click, keyboard navigation,
double-click actions, and context menus remain available. Tree expansion
controls are separate non-selectable command buttons.

Selectable photo-list filenames support native drag and double-click text
selection. When text selection wins, any pending single-click navigation is
cancelled; ordinary single-click and double-click photo actions remain
available when no text is selected.

Native desktop context menus are disabled across the application. Every main
full-photo view uses the Vividarium Photo Context Menu; the Map preview remains
a lightweight preview without a photo context menu.

## Public pages and components

### `PhotoBrowser(props)`

Parameters include a cursor page controller, optional title, and
`PhotoOpenHandlers` for opening details, fullscreen, taxonomy, or mapping. Returns a
virtual list or grid with a shared photo stage. The active photo publishes a
current-photo display state. Stable matched photos show their lightweight
taxonomy display path in the status bar. Unmatched, ambiguous, and processing
photos show their mapping status instead. `TaxonDisplaySummary` remains
taxonomy-only; mapping status is carried separately by the current-photo
display state.

Every main current-photo pane uses one shared `PhotoPaneHeader` presentation:
the filename on the first line and formatted file size and modification time
on the second line, with consistent typography and spacing.

### `PhotoSet(props)`

Parameters: either a search `query` with an optional transient `refreshKey`, or
a `taxonId`, plus `PhotoOpenHandlers`.

Returns: a cursor-backed `PhotoBrowser` for global search or one taxon. A
changed search refresh key clears the saved page and reloads from the first
result.

Photo Browser list and media panes, Folder and photographed-taxonomy tree and
preview panes, and Photo Detail media and metadata panes are adjustable within
page-specific minimum widths. Their divider positions belong to the tab view
state.

### `FolderPhotosView(props)`

Parameters: `handlers: PhotoOpenHandlers` and the optional active background
photo operation.

Returns: an expandable directory tree and photo stage for the active library.
Directory lists use cursor pages and the photo grid is virtualized. Nearing the
loaded page boundary requests the next page; thumbnail media is requested only
near the visible grid area.

### `TaxonPhotosView(props)`

Parameters: `handlers: PhotoOpenHandlers` and the optional active background
photo operation.

Returns: an expandable photographed-taxonomy tree and photo stage. Tree rows
and every clickable breadcrumb node use the Photos visible-name preference.

### `PhotoMapView(props)`

Parameters include `handlers`, the owning tab's active state, and the optional
active background photo operation.

Returns: a viewport-driven MapLibre page. The first open fits the aggregate
coordinates of all geotagged photos; the tab then retains its center and zoom.
Map requests use the visible bounds and backend cursor, and only returned
markers are mounted. Selecting a marker reuses one bottom-right thumbnail and
filename preview. Selecting another marker replaces its contents, selecting
the map closes it, and selecting the preview opens Photo Detail.

Only the selected marker publishes current-photo display state to the status
bar. Browsing the map viewport does not request current-photo state.

Metadata progress invalidations are coalesced into periodic page reloads. While
the user has not dragged or zoomed the map, newly discovered GPS bounds may
refit the initial viewport. The first manual map interaction disables automatic
refitting so background work cannot move the user's chosen view.

### `PhotoDetailView({ photo, handlers })`

Parameters: one `Photo`.

Returns: a photo stage and copyable file and EXIF metadata. The heading shows
the filename followed by formatted file size and modification time. The active
view publishes current-photo display state to the status bar. Width, height,
longitude, and latitude are separate detail rows. The photo stage exposes the shared photo context menu,
which loads mapping state on demand when opened.

### `PhotoStage` and `PhotoThumb`

Parameters: a `Photo`, with display options appropriate to the full image or
thumbnail. Returns media UI using the desktop photo URI, active Photo Library
UUID, and file identity. This prevents cached media from crossing library
boundaries.

Full-image zoom and pan use the normal two-dimensional transform path. Pressing
Enter in image mode enters native fullscreen. Fullscreen returns to its invoking
page and display mode. List-backed photo views restore keyboard focus to their
owning photo list after native fullscreen exits. The fullscreen presentation
owns keyboard focus while active, so background photo navigation does not run;
the selected photo and invoking display mode are preserved. It remains mounted
until the window reports a resize outside fullscreen, when list focus is
restored. Double-clicking a photo item opens fullscreen; double-clicking an
already displayed full image continues to toggle zoom. Photo context menus place
`View fullscreen` before `View photo details`.
Fullscreen presentation is a pure photo-viewing mode: photo context menus are
disabled while it is active, while wheel zoom, double-click zoom, pan, and
Escape-to-exit remain available.

Folder, photographed-taxonomy, and Map pages render a non-blocking indexing
notice while relevant background work is active. Existing results stay
interactive. An initial empty pane has a pane-local Loading state; refresh and
pagination keep existing rows visible.

Folder and photographed-taxonomy views do not force keyboard focus into either
pane when opened or after breadcrumb navigation. Pressing Up or Down outside
the list enters the left browser. If a photo is already selected, navigation
resumes from that photo. Otherwise Down selects the first photo and Up selects
the last photo.
Returning from full-image mode with Escape restores keyboard focus to the left
browser at the current selection.

Folder browsing reports direct folder and photo counts for the current
directory. Photographed-taxonomy browsing reports direct child-taxon and
direct-photo counts for the current taxon. Map browsing reports the number of
currently loaded photos in the visible map area through the status bar and does
not duplicate that count in the map overlay.

### `EmptyWorkspace(props)`

Parameters: the recent-search list, recent-search mutation callbacks, a search
submitter, suggestion availability, and an input ref.

Returns: the no-tab workspace containing only photo search, current-query
suggestions, and recent searches when the query is empty.

### `GlobalSearchOverlay(props)`

Parameters: a search submitter, suggestion availability, and a close callback.

Returns: a modal photo-search input with current-query suggestions. Escape or
a pointer action on the backdrop closes it.

### `PhotoSearch(props)`

Parameters: a `PhotoSearchController`, suggestion availability, an ID prefix,
and optional focus configuration.

Returns: the shared keyboard-accessible search input and suggestion list used
by `EmptyWorkspace` and `GlobalSearchOverlay`. Arrow keys select suggestions;
Enter submits the selected suggestion or normalized input. A pointer action
outside the search closes suggestions. Each suggestion shows available
scientific, Chinese, and English names on the first line and rank on the
second line.

### Search state helpers

`useSearchSuggestions(query, enabled)` waits for a 260 ms input pause, then
returns photo-backed taxon suggestions and a loading flag. Suggestion results
render as a low-priority update, and only the latest request may update them.

`usePhotoSearch(onSubmit)` returns the controlled query, submission error,
query setter, and asynchronous submit function.

Submitted photo searches show a centered searching status until the first
result page resolves.

`loadRecentSearches()`, `saveRecentSearches(searches)`,
`addRecentSearch(searches, query)`, and `removeRecentSearch(searches, query)`
manage the bounded, most-recent-first search list in browser-local storage.

## Interaction contract

`PhotoOpenHandlers` contains callbacks for opening details, fullscreen,
taxonomy, and the mapping editor. `usePhotoInteraction` loads lightweight
mapping state when a context menu opens and routes context actions through
those handlers. Its Photo Context Menu actions are `View fullscreen`, `View
photo details`, taxonomy actions, and mapping actions in that order.

`emitPhotoMutation(mutation)` broadcasts a committed photo or mapping change.
`usePhotoMutation(listener)` receives it immediately.
`useDeferredPhotoMutation(listener, active)` defers refresh work while a view
is hidden and delivers one invalidation when it becomes active.

`usePhotoTaxonDisplaySummary(photoId)` caches lightweight taxonomy summaries
for the current view. A missing selection performs no query. Selecting another
photo loads only that photo, repeated selection reuses the cache, and mapping
or taxonomy mutations invalidate affected values. Status paths stay on one line
and give the finest node the highest space priority.

`usePublishedPhotoTaxonSummary({ photoId, active, onChange })` connects that
on-demand current-photo display state to the status bar. Photo Browser, Folder, photographed
taxonomy, Photo Set, Photo Detail, Mapping, standalone Mapping Editor, and
Map views use it for their current photo. Embedded Mapping Editor delegates
publication to its Mapping page.

The photo context menu places the current mapping status beside `View taxon
details`. The action is enabled only for a stable mapped taxon; the status
remains visible when the action is disabled. Its actions are `View photo
details`, `View taxon details`, `Edit mapping`, `Remap from filename`, `Rename`,
`Rename from taxonomy`, and `Reveal in Finder / Explorer`.

Rename from taxonomy is an explicit action that reads the latest saved filename
format. It reports the new filename after a change and reports when the current
filename already matches. Saving filename-format settings never renames
existing photos.

Non-cancellable photo and directory context-menu mutations lock manual menu
and rename-modal dismissal while running. Tab-owned background tasks remain
cancellable by closing their owning tab.
