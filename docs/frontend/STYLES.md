# Style Modules

Location: `apps/desktop/src/styles`

`index.css` is the only stylesheet imported by `main.tsx`. It composes the
theme, application shell, shared UI, and business-domain styles.

| File | Ownership |
| --- | --- |
| `theme.css` | Color variables, typography, reset, focus behavior, and Light/Dark editor palettes. |
| `app.css` | Desktop shell, activity bar, tabs, toolbar, native About, popovers, and status bar. |
| `shared.css` | Buttons, virtual collections, segmented controls, editor, modal, loading, and empty states. |
| `photos.css` | Photo search, empty workspace, browsers, media, folder tree, detail view, map, and photo context menu. |
| `mapping.css` | Mapping workspace, state badges, candidates, and editor layout. |
| `taxonomy.css` | Taxon cards, taxonomy pages, formatted input, SQL results, scrollable SQL schema sidebars, SQL Import, and Direct Import. |
| `operations.css` | Operation summaries, audit rows, and history actions. |
| `settings.css` | Settings navigation, storage, libraries, taxonomy databases, naming, map, and hook-test layouts. |

Feature components use domain class names and place new rules in their owning
domain file. A rule belongs in `shared.css` only after the corresponding UI is
used by more than one feature domain.

## Shared button interface

`Button` and `IconButton` are exported from `apps/desktop/src/shared/ui.tsx`.

`Button` accepts native button attributes plus `variant` (`primary`,
`secondary`, or `ghost`) and `size` (`default` or `small`). It returns a native
button with the shared visual and interaction states.

`IconButton` accepts the same parameters and requires an `aria-label`. It
returns a square button with the shared icon click target and accessible name.

Button colors, radius, focus ring, transition duration, pressed transform, and
disabled behavior are global. Domain styles may define layout and selected-row
states, but do not redefine ordinary button interaction states.

Application chrome, context menus, overlays, and CodeMirror use shared theme
tokens in Dark theme, explicit Light theme, and system Light mode. Photo media
canvas backgrounds and map marker contrast colors remain independent visual
surfaces.

Semantic success and danger foreground colors use shared theme tokens in every
theme mode.

### Text selection

Command controls (`button` and `.button`) are non-selectable. Content surfaces
that display persistent application data remain text-selectable even when click
or keyboard activation opens that record. Temporary interactions such as panel
resizing may disable selection only for the duration of the drag.
