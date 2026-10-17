# Elettrico

This fork adds an electrical drafting workspace to CADCraft. Keep the implementation
in `crates/ui-egui/src/elettrico.rs` and register its actions in the existing command
and menu infrastructure so upstream updates remain manageable.

## First native interface

- Elettrico → Componi scatola, also Ctrl+Shift+E on Windows.
- Supports with 3, 4, 6 or 7 slots; provisional component sizes match the external prototype.
- UNEL occupies two slots. Adding components or reducing capacity cannot overfill a box.
- Optional installation height stays in the main window.
- A separate Settings window holds print scale and symbol size in 5% steps from 5%.
- A live CAD preview shows the actual external symbols, white on a dark background,
  fitted and centered in their occupied slots. The optional height appears above the box.

Its composition and settings currently last for the application session only.

## Native placement

`Inserisci nella planimetria` prepares a provisional assembly. Pick its point in model space,
then choose rotation in the internal window. With an installation height, a second window
chooses above, below, right or left in drawing coordinates. Its text stays horizontal during
this insertion workflow. Cancel before confirmation leaves the drawing unchanged.

Confirmation writes a DXF backup of the entire current drawing, including unsaved edits,
then adds one block on `EL_SCATOLE`. One Undo removes the entire operation. Backup failures
abort insertion. `CADCRAFT_ELECTRICAL_BACKUPS` selects the backup folder; Windows otherwise
uses `%LOCALAPPDATA%/CADCraft Elettrico/backups`. Locked, frozen or hidden `EL_SCATOLE`
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

Stars toggle favorites, saved atomically in `preferiti.json` beside the catalog:
`{"version":1,"favorites":["switch"]}`. The favorites list is independent of category paths
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
