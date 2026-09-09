# Application Module

Location: `apps/desktop/src/app`

The application module assembles feature domains into the desktop window. It
owns the activity bar, toolbar, native application-menu events, tab instances,
active Photo Library, global search, navigation history, and operation-status
display.

## Public interfaces

### `App()`

Location: `apps/desktop/src/App.tsx`

Parameters: none.

Returns: the React application tree containing `DesktopShell`.

`App` is the composition entry point and contains no page or domain logic. It
suppresses the native WebView context menu for the entire application
lifecycle, including startup loading. Feature-owned custom context menus still
receive their context-menu events and render normally.

### `DesktopShell(props)`

| Parameter | Type | Description |
| --- | --- | --- |
| `generalSettings` | `GeneralSettings` | Current application-wide settings. |
| `onGeneralSettingsChange` | `(settings) => void` | Applies an updated settings value. |
| `generalSettingsLoadError` | optional string | Makes startup load errors visible in General settings. |

Returns: the complete desktop workspace.

The shell creates tab instances and supplies domain pages with navigation and
status callbacks. A taxon tab has an instance ID independent of its current
taxon, allowing in-tab navigation without conflating the tab with a taxon ID.
All tabs may be closed; in that state the shell has no active tab and renders
the Photos domain's `EmptyWorkspace` search entry.

### `closeTabState(tabs, activeId, closingId)`

Parameters: the current tab list, the active tab ID or `null`, and the tab ID
to close.

Returns: the remaining tabs and their next active tab ID. Closing the final
tab returns an empty list and `activeId: null`.

`closeAllTabsState()` returns an empty tab list and `activeId: null` for the
native Close All Tabs action.

Status-bar messages are stored by tab ID. `updateTabStatus` changes only the
reporting tab, `getCurrentTabStatus` returns the active tab's latest message or
`Ready`, and `pruneTabStatuses` removes messages for closed tabs. Switching
tabs therefore never carries folder counts, SQL results, or update results into
another tab.

### Navigation history

`createNavigationHistory(tabId)` creates history for the initial tab.
`recordNavigation(history, tabId)` returns history containing the new target.
`findNavigationTarget(history, direction, openTabIds)` returns the next open
target and updated history. `pruneNavigationHistory(history, openTabIds)`
returns history with closed targets removed.

### `useOperationObserver()`

Parameters: none.

Returns: the latest `OperationsStatus` plus observer state used by the shell.
The status is keyed by task identity, so one module can retain distinct task
records while Background remains the single source of truth. The observer
publishes incremental photo/index/metadata invalidations and successful
completion invalidations. It registers `operation-progress` before relying on
live updates and fetches a status snapshot only for startup, window-focus or
visibility recovery; Background progress does not use a fixed polling loop.

`BackgroundTasks` owns the bottom-right task popover. Active queued and running
tasks appear before a Recent section containing at most ten completed or failed
tasks ordered by finish time. Task titles, stage labels, progress units,
determinate and indeterminate modes, byte counts, and elapsed or finished
duration are defined by `backgroundPresentation`. Running elapsed time updates
once per second in the frontend. The status-bar count includes only queued and
running tasks.

Foreground long-running workflows keep the exact operation handle returned by
their command and wait by `task_id`. Custom SQL, Custom SQL export, Formatted
Update preview and apply, SQL Import validation and apply, and Direct Import
inspection and apply use this path. Owner cancellation remains tied to the tab
while task lifecycle, progress, result, and error remain visible in Background.

### `useNativeMenu(handler)`

Parameters: a handler receiving one typed native `File` menu action.

Returns: nothing. The hook subscribes the shell to Tauri menu events and
removes the event listener when the shell unmounts.

## Feature integration

The shell imports top-level pages from `features/*`. It passes callbacks for
opening photo details, taxonomy records, mapping editors, and Photo Sets. Page
components do not create or manage application tabs directly. The global
photo-search action opens a modal search overlay while a tab is active. In an
empty workspace the same action focuses the existing empty-workspace input.
The native `File` menu opens or manages Photo Libraries and Taxonomy Databases,
or closes every tab. Management actions select the corresponding Settings
section; closing all tabs renders `EmptyWorkspace`.

The native About menu action opens `NativeAboutOverlay`, which returns only
the product name, software version, author, email, and GitHub link. External
links use the system opener and report failures inside the overlay.

Photo Library activation records its returned indexing operation without
waiting. Existing photo data remains browseable while Photo Scan, Metadata
Index, and Photo Mapping run in Background. Folder, Taxon Tree, and Map show a small
incomplete-data notice when their results are still being filled; they are not
covered or disabled. Progress invalidates only affected cursor pages and Map
continues adding newly discovered geotagged photos. Foreground pagination and
pane loads use local loading states and remain outside Background.

Submitting a global photo query always starts a fresh search, including when a
tab for the same normalized query is already open. The existing tab is focused
and its transient refresh key advances; the refresh key is not persisted in
workspace state.
