# Electrical catalog symbol editor

The general catalog manager and the unifilar insertion gallery offer **Modifica
simbolo in nuova scheda**. The selected block opens as ordinary editable CAD
geometry in a separate drawing tab. The originating project remains open and
unchanged. Only one catalog editor is allowed per session.

The editor palette defines an insertion origin and optional IN and OUT points by
clicking in the drawing. These are real Point entities with stable logical names;
CAD Move, Erase and Undo work on them. Colored labels are an editor overlay.
Logical points are omitted from the published block, including from previews.
Other pre-existing port types, such as PE, are retained but do not yet have an
editing control. Connection direction and text-anchor authoring are future work.

**Salva come nuovo simbolo (copia)** is selected initially. Saving creates a new
catalog identity and independent block. Unchecking it replaces only the selected
entry, retaining its identity; sibling variants sharing its previous block remain
untouched. Editing the geometry does not change modules, categories, favorites or
the category-specific device fields. Existing instances in projects remain
unchanged; updating them from the catalog is a separate future feature.

The origin is subtracted from geometry and connection coordinates when saving.
Unifilar placement already uses these local port coordinates. General standalone
placement accepts an optional `origin` from the catalog's `editor_origin`, while
older entries retain their normalized bounding-box anchor behavior. Composer
geometry continues to fit components into their module slots.

Publication writes a complete new version of the DXF library beside the catalog,
then atomically replaces the catalog JSON to reference it. The old library is
retained and the old catalog is backed up under `backups/`. Exact catalog and DXF
byte comparisons refuse concurrent changes. The written DXF is reread before
publication. A failed save leaves the editor open. A crash before publication can
leave an unreferenced revision file but never a catalog pointing at an unfinished
library. This is optimistic conflict detection, not an interprocess database lock.

Saved geometry bounds and editor revision are recorded, stale geometry hashes
are removed, affected general previews are rebuilt, and the matching manager is
invalidated. Reopening the unifilar gallery reloads its symbols and previews.
Favorites resolve through stable IDs. Editing a unifilar entry never invalidates
an unrelated unsaved general-manager draft.

Ctrl+S in the editor uses the palette's catalog save operation. The editor's tab
close asks to discard or continue. Save/close returns by the project's unique ID,
not a potentially shifted tab index. Closing the application also supports saving
the editor through its normal unsaved-changes flow.

Engine/control commands (JSON calls do not open dialogs):

- `electrical.library.open {catalog, id}` opens the independent drawing.
- `electrical.library.read {}` returns the editor identity and point coordinates.
- `electrical.library.point {point: origin|in|out, at: [x,y]}` changes a point.
- `electrical.library.choose {point}` chooses the interactive point role;
  starting `electrical.library.point` then asks for its location.
- `electrical.library.save {name?, copy?: true}` publishes and returns.
- `electrical.library.cancel {discard?: false}` refuses to discard unsaved work
  unless explicitly authorized with `discard: true`.

The native UI wrappers `ui.elettrico.library.open` and
`ui.elettrico.library.save` also update palette state and invalidate UI caches.

Limits: 20,000 top-level geometry objects, coordinate extent ±1000, at most
512 unifilar symbols or 2048 general components, output library 16 MiB, general
catalog 1 MiB and unifilar catalog 2 MiB. Only local desktop catalogs are supported.
Tests use original synthetic geometry and cover project preservation, independent
copies, logical coordinates, Undo, validation, concurrent modification, cancel,
and variants sharing a source block. Private libraries remain outside Git.
