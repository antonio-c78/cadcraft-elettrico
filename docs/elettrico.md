# Elettrico

This fork adds an electrical drafting workspace to CADCraft. Keep the implementation
in `crates/ui-egui/src/elettrico.rs` and register its actions in the existing command
and menu infrastructure so upstream updates remain manageable.

## First native interface

- Elettrico → Componi scatola, also Ctrl+Shift+E on Windows.
- Boxes with 3, 4 or 6 slots; provisional component sizes match the external prototype.
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
legends; those tools are not implemented yet. Rotating a completed block with the generic
ROTATE command rotates its height too; independent annotation editing is a later feature.

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

`library` resolves relative to the JSON file. `modules` is occupied slots (1–6); unknown
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
2. Edit an existing assembly from its stored composition metadata.
3. Build material schedules and a legend of used symbols.

User drawings and third-party symbol libraries stay outside this public source repository.
The material model will distinguish the flush box, support, devices, blanking modules and plate.
