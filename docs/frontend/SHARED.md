# Shared Module

Location: `apps/desktop/src/shared`

Shared contains code with established use across multiple feature domains. It
does not own business data or import feature modules.

## UI interfaces

`VirtualList<T>` and `VirtualGrid<T>` render windowed collections. Their
parameters include items, dimensions, a stable item key, a renderer, and an
optional end-of-list callback. `VirtualList` clamps persisted scroll state when
the collection shrinks; consumers may provide `resetKey` to return to the first
row when a query or filter changes.

`SectionHeader`, `EmptyState`, `Modal`, `Segmented`, `Disclosure`, and `Busy`
provide domain-neutral presentation. `CodeEditor` provides the common SQL and
Rhai editing surface with syntax highlighting. `Modal` defaults to dismissible
through its close action, Escape, and backdrop. Non-cancellable mutations pass
`dismissible={false}` while running, which disables all three dismissal paths
until the owning interaction completes.

`ResizablePanels(props)` returns two panes separated by a pointer- and
keyboard-adjustable divider. Parameters select horizontal or vertical layout,
initial size or ratio, minimum pane sizes, an accessible separator label, and
an optional `stateKey`. A keyed divider stores both horizontal and vertical
sizes in the owning tab's view-state store, so a non-keep-alive page restores
its layout after remounting.

### Selectable content activation

Selectable content surfaces keep native text selection while supporting record
activation. Activation is suppressed only when the current selection intersects
that surface; a selection elsewhere does not block navigation. Standalone
surfaces provide keyboard activation, while virtualized rows use the
`VirtualList` keyboard model.

## Cursor interfaces

`useCursorPage<T, P>(options)` accepts initial parameters and a loader returning
`Page<T>`. It returns `items`, `cursor`, `loading`, `error`, `hasMore`, `load`,
`reload`, and controlled item setters.

`useCursorTree<T, K>(options)` accepts a node key and child-page loader. It
returns expansion state, cached children, loading state, and load-more actions
for expandable folder and taxonomy trees.

## View-state interfaces

`ViewStateProvider({ store, children })` supplies a tab-owned state store.
`useViewState(key, initialValue)` returns a React state pair whose current value
survives page unmounting. Each tab instance receives its own store, preventing
state from leaking between tabs of the same kind.

## Metadata notification

`emitMetadataChange(change)` and `useMetadataChange(listener)` carry committed
metadata invalidations between Settings and the feature domains that consume
those values.
