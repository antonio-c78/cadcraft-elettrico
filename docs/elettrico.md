# Elettrico

This fork adds an electrical drafting workspace to CADCraft. Keep the implementation
in `crates/ui-egui/src/elettrico.rs` and register its actions in the existing command
and menu infrastructure so upstream updates remain manageable.

## First native interface

- Elettrico → Scala da misura nota is the first visible electrical menu item and
  toolbar button, with an original CADCraft-style dimension/scale icon.
- Elettrico → Componi scatola, also Ctrl+Shift+E on Windows.
- Supports with 3, 4, 6 or 7 slots; provisional component sizes match the external prototype.
- UNEL occupies two slots. Adding components or reducing capacity cannot overfill a box.
- Optional installation height stays in the main window.
- A separate Settings window holds print scale and symbol size in 5% steps from 5%.
- A live CAD preview shows the actual external symbols, white on a dark background,
  fitted and centered in their occupied slots. The optional height appears above the box.

Its composition and settings currently last for the application session only.

## Prepare a plan from a known distance

`Scala da misura nota...` opens one guided window: choose the whole model-space
plan or the current CAD selection, specify model units (mm/cm/m), and pick two
distinct points using normal CAD snaps. Prepared objects are highlighted. Nothing
in the drawing changes while measuring or cancelling. The first point is the fixed
scale base; the real length accepts a decimal comma and an independent mm/cm/m unit.
The confirmation shows the measured length, uniform factor, target count and any
included native electrical boxes, symbols or rooms. Model units are taken from
INSUNITS when supported; otherwise the user must choose them explicitly.

Applying calls the same native transform as SCALE after complete target validation
and a successful DXF backup. Locked targets reject the entire operation rather
than silently leaving part of the plan unscaled; hidden but unlocked model objects
are included in whole-plan mode. Paper-space entities and shared block definitions
are not transformed. The chosen INSUNITS and the geometry are one undoable action.
Prepared handles, document UID and revision are pinned, so changing selection does
not redirect an operation and drawing changes require repeating the measure.
Lengths, coordinates, factors and selection size are bounded; zero reference
lengths, invalid units, duplicate/missing handles and stale plans are rejected.

After applying, the same window offers a read-only two-point check with optional
expected length and a displayed difference. It never rescales the drawing. The
original selection is restored on closing. Calibrate before defining rooms or
inserting electrical components whenever possible, since included native objects
are scaled just like any other selected geometry.

Engine commands are `electrical.calibration.prepare` (scope/selection snapshot),
`electrical.calibration.pick` (provisional interactive two-point measurement), and
`electrical.calibration.apply` (explicit points, length, units, handles and snapshot).
JSON execution never opens the guided window.

## Native placement

`Inserisci nella planimetria` prepares a provisional assembly. Pick its point in model space,
then choose rotation in the internal window. With an installation height, a second window
chooses above, below, right or left in drawing coordinates. Its text stays horizontal during
this insertion workflow. Cancel before confirmation leaves the drawing unchanged.

Confirmation writes a DXF backup of the entire current drawing, including unsaved edits,
then adds one block on the configured components layer (default `EL_COMPONENTI`). One Undo removes the entire operation. Backup failures
abort insertion. `CADCRAFT_ELECTRICAL_BACKUPS` selects the backup folder; Windows otherwise
uses `%LOCALAPPDATA%/CADCraft Elettrico/backups`. Locked, frozen or hidden component layers
layers must be enabled by the user first. No existing drawing file is overwritten.

Settings includes model units (mm or m). Drawing factor is print scale × percentage / 100,
divided by 1000 for meters. The box is 3 paper mm per slot and 5 paper mm high before
applying the percentage. Verify model units before placing in an imported plan.

Catalog graphics are cached as vector strokes in the assembly (curves are tessellated;
catalog text is represented by its rendered strokes). The installation height remains CAD
text. Each outer INSERT stores UTF-8 JSON in `CC_COMPOSITORE` XDATA, preserving component
names, source block identifiers, occupied slots, factor, height, placement rotation and label
side. Metadata survives DXF saves, and is groundwork for editing, material schedules and
legends. Material schedules and legends are not implemented yet. Generic ROTATE rotates
the whole block; reopening it through the electrical editor restores horizontal annotation.

Engine commands: `electrical.prepare`, interactive `electrical.place`, `electrical.options`,
and dialog-free `electrical.insert`. The latter accepts the external library, composition,
point, rotation, side and optional backup directory. Preparation never changes the document.

The local launcher supplies the external JSON path through `CADCRAFT_ELECTRICAL_CATALOG`.
`ui.elettrico.catalog {path}` loads or reloads it without changing a drawing. A call with no
path reloads the current file (or the environment path after an initial load error). Settings
offers `Ricarica catalogo e simboli` and a DXF override picker through `ui.elettrico.library`.
DXF reads are bounded to 16 MB; previews are cached until a reload. Source artwork and the
user's actual metadata stay outside this repository.

## External catalog and favorites

The catalog is UTF-8 JSON (optional BOM), limited to 1 MB and 512 enabled box components:

```json
{
  "version": 1,
  "library": "symbols.dxf",
  "defaults": {"scale": 50, "size_percent": 100, "units": "mm"},
  "components": [
    {"id": "switch", "name": "Switch", "block": "MySwitch",
     "modules": 1, "category": ["Controls", "Switches"]}
  ]
}
```

`library` resolves relative to the JSON file. `modules` is occupied slots (1–7); unknown
occupancy must be reviewed before adding a symbol to the enabled catalog. `category` is
an optional path of up to four names, creating nested expandable menus. Names, graphics,
slot occupancy, categories, default scale, default percentage and default units can change
without rebuilding or restarting CADCraft: edit the external files and reload in Settings.
Drawing dimensions remain the paper dimensions documented above; occupied slots are the
configurable component footprint. Temporary drawing settings are preserved on a name-only
reload; changed default fields are applied individually.

IDs are unique and stable: keep them when renaming or moving a component. Reload remaps
the current composition by ID, so reordering cannot substitute devices. Removing a selected
component or increasing occupancy beyond capacity rejects the reload and retains the old
catalog and composition. Invalid JSON, failed DXF reads and invalid preferences also retain
the previous configuration. End an active drafting command before reloading.

Stars toggle favorites, saved with a backup in the version-2 catalog JSON. Legacy
`preferiti.json` files are read until the first catalog/favorites save. The favorites list is independent of category paths
and names; IDs absent from the current catalog remain stored for later catalogs. Search
matches component names or source block names. No reload changes previously placed boxes.

The user's recovered-symbol index is a separate external archive. It is not automatically
enabled as box components: categories, suitability and occupancy require human review.
Full archive browsing and standalone-symbol insertion are later features.

## Fast iteration

Use the development executable in `target/debug/cadcraft.exe` for interface review. The
local trial build uses `cargo build --workspace --bins`, sharing the workspace feature set
and dependency cache with CI. Optimized release builds are reserved for finalized groups
of changes. The full CI gate still runs before committing a completed feature.

## Next implementation stages

1. Load configurable component metadata, including slot occupancy.
2. Expand the electrical editor and review series-specific compatibility with the owner.
3. Build material schedules and a legend of used symbols.

User drawings and third-party symbol libraries stay outside this public source repository.
The material model will distinguish the flush box, support, devices, blanking modules and plate.

## Edit an existing box

Elettrico → Modifica scatola accepts one preselected native composer block, or asks for
one click in the drawing. Creation and complete editing use the same composer window,
including categories, favorites, component rows and preview. Editing adds a top dropdown:
whole box, installation height, or label position. Partial modes show only their controls
and preview, and apply only that field from the original snapshot, even if a full draft has
unconfirmed changes. Returning to whole-box mode retains the draft. The old separate editor
and single-component mode are removed. Select a component in the preview or use Sostituisci,
then choose its replacement in the same catalog, preserving the component's list position.
Complete editing allows changing capacity (3, 4, 6, 7), rebuilding from an empty composition,
and changing model size through the shared Settings window. Exact saved model factor, position and
rotation are retained unless explicitly changed. Riposiziona nel disegno opens placement
review for point and rotation. Overfull and empty compositions cannot be confirmed.
Six and seven slots describe support capacity; they do not imply a different flush box.
Physical compatibility with civil series remains a future, explicitly reviewed catalog rule.

Apply replaces only the chosen reference, retaining its handle and outer properties. A fresh
block definition protects copied references sharing the old block. Original geometry remains
in the drawing until confirmation, with a DXF backup before the atomic change and one Undo.
Closing or canceling either window does not alter the original. A changed document/revision
requires reopening the editor. Complete placement review keeps the current point initially;
`Scegli un nuovo punto` permits repicking before rotation and label confirmation.
The new-box draft and settings are restored after edit confirmation or cancellation.
Historical component names and footprints remain in the draft even after a catalog change;
temporary entries are removed when editing ends. Catalog reload is disabled during editing.

Height/label/placement-only edits reuse the stored vector geometry without the external DXF.
Composition or size changes rebuild using the current symbol library. Preview in the editor
shows composition; the placement review shows actual drawing rotation and label side.
Moved, rotated and positively, uniformly scaled native references are supported; arrays,
mirroring, nonuniform scaling and nonzero block bases are rejected rather than guessed.
Older external-composer blocks without native version-1 metadata require conversion first.

Dialog-free commands: `electrical.read {handle}` returns the snapshot including `uid` and
`revision`; pass this snapshot with changed fields to `electrical.update` for a single
transaction. `electrical.edit.prepare` prepares the same change without modifying the
drawing, then interactive `electrical.place` reviews placement before confirmation.

## Symbol catalog manager

Elettrico → Gestione catalogo simboli edits a separate draft, with category browsing,
search, a white-on-dark geometry preview, names, category paths, module occupancy, notes,
and favorites. Duplicate creates a new stable identity sharing the original geometry;
its properties are independent. Deleted variant IDs are not recycled after saving.
Deleting only removes the catalog entry, not source geometry or existing drawing blocks.
Closing keeps the unsaved draft; explicit reload discards it.

Two independent flags, `composer` and `standalone`, default to true for version-1 catalogs.
Composer-only filtering is active immediately after saving. Standalone availability filters the insertion gallery. Folder import remains an explicit
preparation step. A standalone-only entry may have zero
modules. All composer entries still require reviewed occupancy of 1–7 modules.

Save validates the complete draft, source geometry and remapping of any current composition
before writing. It refuses removal/disablement of selected components and overfilling the
current box. A concurrent file change refuses the save. The exact original JSON is backed
up under the catalog's `backups` directory before replacing it with a single version-2
file containing both properties and favorites. Unknown metadata fields are preserved.
Existing boxes are never regenerated by a catalog edit. Changes apply without compilation
or restart. End an active drawing command or box edit before saving.

`ui.elettrico.manager` supports dialog-free JSON actions `read`, `select`, `update`,
`duplicate`, `delete`, `save`, and `reload`. Mutations target `id`; `update` accepts `fields`
(name, category, modules, composer, standalone, notes) and optional `favorite`. These act on
the draft until explicit `save`. The normal menu opens the window.

### Visual category gallery

The manager opens with a contact sheet and a category-only sidebar. Selecting a parent
category includes all descendant categories; All and Favorites provide alternate views.
Search filters within the current scope by display name, source block or category path.
A thumbnail-size slider changes the responsive column count. Only visible grid rows are
painted, using the existing cached white vector previews rather than reloading DXF files.
Each card shows its symbol and a wrapped name, with the full name on hover.

Clicking a card opens the existing attribute form. Back restores the category, query,
thumbnail size and that view's stored scroll position. Browsing never saves or changes
catalog properties. Duplicate opens the new variant's details. Save/backup behavior is
unchanged. The complete organized library can replace the initial examples without duplicate entries.

`ui.elettrico.manager {action:"browse",category:[...],search:"...",favorites:false}` selects
a gallery scope; `action:"back"` closes detail. Read replies include view, selected stable
ID, category, query and visible IDs, allowing navigation to be exercised without dialogs.


### Full organized library

The active catalog supports up to 2,048 entries/favorites within the existing 1 MB JSON
limit; preview libraries retain the 16 MB DXF limit. The duplication bound uses the same
shared entry limit. The owner's complete import contains 654 organized general symbols,
including the former examples once each. Known entries retain stable IDs, module counts,
composer flags and favorites; other entries start outside the composer with zero/unreviewed
occupancy. Names and category paths come from the organized manifest, including Varie.

Private source DXFs, organized individual files and import manifests stay outside Git.
Before activation, verify source file hashes and all native previews in a separate session.
Store an exact backup of the old JSON, use a versioned DXF copy, then replace the active JSON
only if its original hash still matches. This changes the catalog, never placed drawing
blocks. Source files remain available for later review and reimport.


## Standalone symbol insertion

Elettrico → Inserisci simbolo reuses the manager category/gallery renderer and cached
white vector previews, but filters by `standalone`. Clicking a card starts insertion,
never the attribute form. The source catalog and any unsaved manager draft are preserved.
The gallery's Settings copies composer print scale, size percentage and model units each
time it is opened from the menu. Changes then stay local to standalone insertion; a reset
button copies current composer values again. Size percentages run from 5 to 300 in steps
of 5. Returning after insertion preserves the current gallery browsing context/settings.

Inside CADCraft: pick the anchor point, confirm orientation, enter optional height in cm,
then choose above/below/right/left with a live preview and confirm. Blank height skips the
label-position step. The height label stays horizontal during placement. Dimensions fit
source geometry proportionally into the catalog paper width/height (default 8 mm each),
using the catalog normalized anchor (default center). No box frame or slot separators
are added. Only the final confirmation creates one block on EL_COMPONENTI, preceded by a
DXF backup and covered by one Undo. Cancel before confirmation changes no drawing.

CC_SIMBOLO XDATA stores catalog identity, name, source block/category, scale, size, anchor,
rotation and optional height/label side for later editing, legends and material schedules.
Existing box insertion/editing remains on its original workflow and CC_COMPOSITORE schema.
Symbol-specific editing and schedules are future commands, not part of this insertion UI.

Dialog-free commands: electrical.symbol.prepare {library,symbol,backup_dir?}, followed by
interactive electrical.symbol.place; or electrical.symbol.insert with at:[x,y], rotation,
height_cm and side. The symbol object requires id,name,block,category,factor,width_mm,
height_mm,anchor:[x,y]. electrical.options controls rotation/side and symbol height_cm
(null clears it). UI commands ui.elettrico.symbols and ui.elettrico.symbol.insert {id}
use the active external catalog. Settings JSON uses scale,size_percent,units (mm or m).

## Barra strumenti Elettrico

La barra Elettrico, sotto il menu, richiama Componi scatola, Modifica scatola,
Inserisci simbolo e Gestione catalogo. Le icone originali usano la stessa griglia,
palette del tema e interazione delle altre icone CADCraft. Il nome appare al
passaggio del mouse. Durante un comando i pulsanti sono disabilitati; senza un
disegno resta disponibile il catalogo. La barra segue la visibilità generale delle
barre strumenti e scorre nelle finestre strette.

## Layer dedicati

New boxes and standalone symbols share `EL_COMPONENTI`. Their internal geometry uses
layer 0, inheriting the reference's component layer. Installation-height text belongs
to `EL_ALTEZZE` inside the same block: hiding that layer removes only the height;
moving or copying the reference keeps the text attached. Hiding the components layer
hides the whole reference, including its text. Properties use ByLayer, so colors and
lineweights follow the corresponding layers. Existing layer settings are preserved.

`EL_DESCRIZIONI` and `EL_LEGENDA` are also created, ready for future features. All four
names can be changed in the external catalog's root `layers` object, with string keys
`components`, `heights`, `descriptions`, `legend`. Names must be valid, distinct,
nonzero CAD layer names. Missing configuration uses the defaults above. Prepare and
insert commands accept the same optional `layers` object. The names are retained in
the object's metadata so later box edits preserve its configuration.

Hidden height layers are allowed for insertion; locked height layers refuse changes.
Existing drawings are never rewritten on opening. Editing an older native box applies
the new default split to that reference only, with backup and Undo; other references
sharing its original definition remain unchanged. No bulk migration is performed.

## Ambienti

Elettrico → Definisci ambiente (also on the electrical toolbar) starts a provisional
boundary. Click its vertices, then press Enter, click the first vertex or choose
Chiudi perimetro. The final window requests a room name, optional floor and text
height in drawing units. Salva ambiente creates one block reference containing a
closed straight polyline and its label. Cancel leaves the drawing untouched. The
new `EL_AMBIENTI` layer is green and nonplotting by default; existing layer settings
are preserved. The layer can be hidden or enabled for plotting through CADCraft.

Gestione ambienti lists room names, floors and the count of native electrical
objects (a composed box counts as one object). Selecting a single room in the CAD
also selects its manager row and enables editing. A row chosen in the manager
stays selected while components are selected for manual assignment; an unchanged
CAD selection does not override that choice. Modify renames the room or redraws
its perimeter, preserving its reference handle and manual assignments. Copies get
their own handles; changing one reference never edits a shared definition. A label
is positioned inside a concave perimeter. Boundaries are limited to 3–512 vertices;
zero-length sides, retracing, self-intersections and self-touches are rejected.

### Selection context menu

The canvas right-click menu retains its standard CAD actions and adds an
**Elettrico** submenu for native electrical objects in model space. A single
composed box opens the existing composer, with direct shortcuts to height and
height-label position. A single standalone symbol opens the same symbol gallery
for replacement or a small height/label editor. Replacing a symbol preserves its
reference handle, actual factor, insertion point, rotation, height, layer mapping
and room assignment, and never changes other references sharing its old block.
Symbol updates use the same backup-before-mutation and one-step undo transaction
as composed boxes (`electrical.symbol.read`, `electrical.symbol.update`).

For boxes and standalone symbols, **Ambiente di appartenenza...** reports the room,
floor and manual/automatic/unresolved status. The dialog can assign/change the
room, highlight its reference, or open its existing editor. Selected electrical
components can also be reassigned together or reset to automatic assignment.
Dialogs retain their original target handles and document identity/revision, so
changing the CAD selection never redirects an operation to different objects;
document changes require reopening the dialog.

A room's context menu offers the existing editor, perimeter redraw, selection of
its assigned components and confirmed deletion. Mixed selections containing ordinary
CAD objects or rooms and components do not expose incompatible electrical actions;
multi-component selections expose only shared assignment actions. MOVE and ROTATE
remain in the standard CAD menu. Future electrical types should extend this
selection-based command routing and reuse existing editors rather than introduce
parallel editing workflows. No symbol artwork is embedded in the application.

Room identity is the native model-space reference handle, persisted by DXF; room
metadata uses XDATA `CC_AMBIENTE` version 1. Rooms can be moved/copied normally;
reads use the actual transformed perimeter. Nonuniform or mirrored scaling is
rejected. Up to 512 rooms and 20,000 electrical objects are supported, with a bounded
scan budget. Definitions may overlap (including floors drawn on the same plan),
but an ambiguous location is never silently assigned.

Assignments are read from each electrical reference's insertion point, independently
of layer visibility. Strict interior gives automatic membership; boundary, outside,
overlap and missing manually assigned room are reported separately. Select native
boxes/symbols in the drawing, choose a room in the manager and use Assegna selezionati
to override membership. Assegnazione automatica clears that override for the selected
objects. XDATA `CC_AMBIENTE_REF` stores the room handle and survives native box edits.
Refresh the manager to reread positions after CAD edits. These are object counts,
not an exported bill of materials or a count of fruits inside compositions.

All create/edit/delete/assignment commits have a pre-change DXF backup and one Undo.
No client drawing is converted on opening. Geometry and metadata remain in the
drawing, not in the external symbol catalog. Native commands are:

- `electrical.room.prepare {handle?,label_height?,backup_dir?}` and interactive
  `electrical.room.place`; `electrical.room.options` accepts `name`, `floor`,
  `label_height` and `action: close|undo|redraw`.
- `electrical.room.create {points:[[x,y],...],name,floor?,label_height?,backup_dir?}`.
- `electrical.room.list {}` and `electrical.room.scan {}` are read-only.
- `electrical.room.assign {room:handle|null,handles?,backup_dir?}` uses the current
  selection when handles are omitted; null returns to automatic membership.
- `electrical.room.delete {handle,backup_dir?}` deletes only that reference. Existing
  overrides to a deleted room are flagged until reassigned.

## Focused electrical shortcut menu

Right-clicking a selection made entirely of native electrical objects opens a compact
menu with directly accessible actions and noninteractive category headings. Empty
categories are omitted. The stable order is Edit, Move and copy, Connections,
Rooms and assignments, Delete, Information. Ordinary and mixed CAD selections retain
the standard shortcut menu, as do the block editor, paper space and snap menus.
The complete original menu remains under More CAD commands, including its separators
and command availability rules.

`elettrico/context.rs` resolves applicable electrical actions; `context/menu.rs` owns
category order and the small set of common commands; `context_menu.rs` hosts the popup.
New actions reuse the existing dispatcher and default to Edit until categorized.
No drawing edits happen during menu construction. Unifilar selections promote the
native circuit move and connection commands instead of generic transforms that bypass
table data and route dependencies. Room deletion uses its native confirmation flow.


## Unifilar logical-section deletion

For a native wire, **Elimina tratto...** starts a point picker with a highlighted
preview and confirmation. A section runs between electrical connections (device
terminals, real branches, PE/bus attachments or free ends); bends and ordinary
crossings do not split it. Bars retain their separate whole-bar deletion flow.
Escape restores the original selection without changing the drawing.

`connections/sections/graph.rs` builds a bounded physical graph from explicit
attachments and coincident conductor runs. `sections.rs` plans the replacement
chains on a clone, remaps surviving line markers by position, validates routing,
and commits with a pre-change backup and one Undo. Junction graphics are always
recomputed, including redundant coincident terminal runs. New persistent version-3
`start_node`/`end_node` anchors keep split routes independent of deleted wire
handles; they survive DXF round trips and follow circuit column moves. Opening a
drawing does not rewrite existing routes.

Commands: `electrical.unifilar.section.delete {model,handle?,at:[x,y],backup_dir?}`
for automation; `.section.prepare` and interactive `.section.place` for the UI.
The explicit low-level `.link.delete` API retains whole-link semantics.
